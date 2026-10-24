//! The client side of the registry routes, over one PQ-mTLS [`Session`]: upload a signed
//! artefact in chunks, list the registry, download an artefact and re-verify it. Used by
//! the tests, and meant for the release tooling and the agent. [FR-B-1] [FR-B-4]

use bifrost_artefact::{ArtefactManifest, SignedArtefact};
use pqc_agility::{SigId, Signature};
use service_runtime::client::Session;
use service_runtime::json::{parse, Json};

use crate::registry::CHUNK_BYTES;
use crate::{
    b64, outcome, COMMIT_ARTEFACT, GET_ARTEFACT, LIST_ARTEFACTS, PUT_ARTEFACT_CHUNK,
    REGISTER_ARTEFACT, VERIFY_ARTEFACT,
};

/// A call that did not succeed: the service's outcome code and message, or `None` when
/// the transport failed or the reply made no sense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    /// The [`outcome`] code, when the service answered.
    pub code: Option<u8>,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.code {
            Some(outcome::DENIED) => "denied",
            Some(outcome::MALFORMED) => "malformed",
            Some(outcome::NOT_FOUND) => "not found",
            Some(outcome::REJECTED) => "rejected",
            Some(outcome::CONFLICT) => "conflict",
            Some(_) => "failed",
            None => "unreachable",
        };
        write!(f, "{label}: {}", self.message)
    }
}

fn broken(message: impl Into<String>) -> CallError {
    CallError {
        code: None,
        message: message.into(),
    }
}

/// Call `method` with a JSON request and return the accepted reply as JSON.
pub fn call(session: &mut Session, method: &str, request: &Json) -> Result<Json, CallError> {
    let response = session
        .call(method, request.encode().as_bytes())
        .map_err(broken)?;
    let (code, body) = response
        .split_first()
        .ok_or_else(|| broken("the service sent an empty response"))?;
    let body = String::from_utf8_lossy(body).into_owned();
    if *code != outcome::ACCEPTED {
        return Err(CallError {
            code: Some(*code),
            message: body,
        });
    }
    parse(&body).map_err(|e| broken(format!("the reply is not JSON: {e}")))
}

/// Upload `artefact`, signed by the release signer `signer`, and commit it: the
/// registry's listing of it once the gate has passed.
pub fn register(
    session: &mut Session,
    artefact: &SignedArtefact,
    signer: &str,
) -> Result<Json, CallError> {
    let m = &artefact.manifest;
    let begin = Json::obj(vec![
        (
            "manifest",
            Json::obj(vec![
                ("id", Json::s(&m.id)),
                ("version", Json::s(&m.version)),
                ("component", Json::s(&m.component)),
                (
                    "payloadSha256",
                    Json::s(&attest::hex::hex(&m.payload_sha256)),
                ),
                ("sbomSha256", Json::s(&attest::hex::hex(&m.sbom_sha256))),
            ]),
        ),
        ("sbom", Json::s(&artefact.sbom)),
        (
            "signature",
            Json::s(&attest::hex::hex(artefact.signature.as_bytes())),
        ),
        ("signer", Json::s(signer)),
        ("size", Json::Int(artefact.payload.len() as i64)),
    ]);
    let begun = call(session, REGISTER_ARTEFACT, &begin)?;
    let upload_id = begun
        .str_field("uploadId")
        .ok_or_else(|| broken("no uploadId in the reply"))?
        .to_string();
    let chunk = begun
        .get("chunkBytes")
        .and_then(Json::as_i64)
        .map_or(CHUNK_BYTES, |n| (n.max(1) as usize).min(CHUNK_BYTES));
    let mut offset = 0usize;
    for piece in artefact.payload.chunks(chunk) {
        call(
            session,
            PUT_ARTEFACT_CHUNK,
            &Json::obj(vec![
                ("uploadId", Json::s(&upload_id)),
                ("offset", Json::Int(offset as i64)),
                ("data", Json::s(&b64::encode(piece))),
            ]),
        )?;
        offset += piece.len();
    }
    call(
        session,
        COMMIT_ARTEFACT,
        &Json::obj(vec![("uploadId", Json::s(&upload_id))]),
    )
}

/// The registry's listing: `{"signers", "artefacts"}`.
pub fn list(session: &mut Session) -> Result<Json, CallError> {
    call(session, LIST_ARTEFACTS, &Json::obj(vec![]))
}

/// Ask the registry to re-verify `id`@`version` at rest now.
pub fn verify(session: &mut Session, id: &str, version: &str) -> Result<Json, CallError> {
    call(
        session,
        VERIFY_ARTEFACT,
        &Json::obj(vec![("id", Json::s(id)), ("version", Json::s(version))]),
    )
}

/// A downloaded artefact and the key id of the signer the registry verified it under.
/// The caller verifies it again against its own anchor before using it.
#[derive(Debug, Clone)]
pub struct Downloaded {
    /// The artefact, as the registry holds it.
    pub artefact: SignedArtefact,
    /// The release signer's key id.
    pub signer: String,
}

/// Download `id`@`version` slice by slice.
pub fn download(session: &mut Session, id: &str, version: &str) -> Result<Downloaded, CallError> {
    let mut payload = Vec::new();
    let mut first: Option<(ArtefactManifest, String, Signature, String, usize)> = None;
    loop {
        let reply = call(
            session,
            GET_ARTEFACT,
            &Json::obj(vec![
                ("id", Json::s(id)),
                ("version", Json::s(version)),
                ("offset", Json::Int(payload.len() as i64)),
            ]),
        )?;
        let data = reply
            .str_field("data")
            .ok_or_else(|| broken("no data in the reply"))
            .and_then(|d| b64::decode(d).map_err(broken))?;
        if first.is_none() {
            first = Some(parse_header(&reply)?);
        }
        let size = first.as_ref().map_or(0, |f| f.4);
        if data.is_empty() && payload.len() < size {
            return Err(broken("the registry sent an empty slice before the end"));
        }
        payload.extend_from_slice(&data);
        if payload.len() >= size {
            break;
        }
    }
    let (manifest, sbom, signature, signer, size) = first.expect("one reply was read");
    if payload.len() != size {
        return Err(broken(
            "the registry sent more bytes than the artefact holds",
        ));
    }
    Ok(Downloaded {
        artefact: SignedArtefact {
            manifest,
            sbom,
            payload,
            signature,
        },
        signer,
    })
}

type Header = (ArtefactManifest, String, Signature, String, usize);

fn parse_header(reply: &Json) -> Result<Header, CallError> {
    let field = |j: &Json, k: &str| {
        j.str_field(k)
            .map(str::to_string)
            .ok_or_else(|| broken(format!("no {k} in the reply")))
    };
    let digest = |j: &Json, k: &str| {
        attest::hex::unhex(&field(j, k)?)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or_else(|| broken(format!("{k} is not a digest")))
    };
    let m = reply
        .get("manifest")
        .ok_or_else(|| broken("no manifest in the reply"))?;
    let manifest = ArtefactManifest {
        id: field(m, "id")?,
        version: field(m, "version")?,
        component: field(m, "component")?,
        payload_sha256: digest(m, "payloadSha256")?,
        sbom_sha256: digest(m, "sbomSha256")?,
    };
    let alg = field(reply, "alg")?;
    let alg = [SigId::MlDsa44, SigId::MlDsa65, SigId::MlDsa87]
        .into_iter()
        .find(|a| a.as_str() == alg)
        .ok_or_else(|| broken(format!("unknown signature algorithm {alg}")))?;
    let signature = attest::hex::unhex(&field(reply, "signature")?)
        .ok_or_else(|| broken("the signature is not hex"))?;
    let size = reply
        .get("size")
        .and_then(Json::as_i64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| broken("no size in the reply"))?;
    Ok((
        manifest,
        field(reply, "sbom")?,
        Signature::new(alg, signature),
        field(reply, "signer")?,
        size,
    ))
}
