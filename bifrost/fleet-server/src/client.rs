//! The client side of the registry routes, over one PQ-mTLS [`Session`]: upload a signed
//! artefact in chunks, list the registry, download an artefact and re-verify it. Used by
//! the tests, and meant for the release tooling and the agent. [FR-B-1] [FR-B-4]
//!
//! Each call needs its own policy action: [`register`] `RegisterArtefact`; [`list`]
//! `ReadArtefacts`; [`download`] and [`download_verified`] `DownloadArtefact` (agents);
//! [`verify`] `VerifyArtefact`. A call the identity's roles lack is
//! [`outcome::DENIED`](crate::outcome::DENIED), naming the action.

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
        ("sbom", Json::s(&b64::encode(artefact.sbom.as_bytes()))),
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

/// The whole registry listing, every page: `{"signers", "artefacts"}`. Needs
/// `ReadArtefacts`.
pub fn list(session: &mut Session) -> Result<Json, CallError> {
    let mut items = Vec::new();
    let mut signers = Json::Arr(Vec::new());
    let mut cursor: Option<String> = None;
    loop {
        let mut request = vec![("limit", Json::Int(crate::registry::MAX_LIST_PAGE as i64))];
        if let Some(c) = &cursor {
            request.push(("cursor", Json::s(c)));
        }
        let page = call(session, LIST_ARTEFACTS, &Json::obj(request))?;
        if let Some(s) = page.get("signers") {
            signers = s.clone();
        }
        match page.get("artefacts") {
            Some(Json::Arr(more)) => items.extend(more.iter().cloned()),
            _ => return Err(broken("no artefacts in the reply")),
        }
        match page.str_field("next") {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    Ok(Json::obj(vec![
        ("signers", signers),
        ("artefacts", Json::Arr(items)),
    ]))
}

/// Ask the registry to re-verify `id`@`version` at rest now, running the whole gate.
/// Needs `VerifyArtefact`.
pub fn verify(session: &mut Session, id: &str, version: &str) -> Result<Json, CallError> {
    call(
        session,
        VERIFY_ARTEFACT,
        &Json::obj(vec![("id", Json::s(id)), ("version", Json::s(version))]),
    )
}

/// A downloaded artefact and the key id of the signer the registry verified it under.
#[derive(Debug, Clone)]
pub struct Downloaded {
    /// The artefact, as the registry holds it.
    pub artefact: SignedArtefact,
    /// The release signer's key id.
    pub signer: String,
}

/// Download `id`@`version` — its SBOM, then its bytes, slice by slice — as the registry
/// serves it. Needs `DownloadArtefact`. Prefer [`download_verified`], which also checks
/// it against the caller's own anchors before handing it over.
pub fn download(session: &mut Session, id: &str, version: &str) -> Result<Downloaded, CallError> {
    let (header, sbom) = fetch_part(session, id, version, "sbom")?;
    let (again, payload) = fetch_part(session, id, version, "payload")?;
    if again.manifest != header.manifest || again.signer != header.signer {
        return Err(broken("the artefact changed during the download"));
    }
    let sbom = String::from_utf8(sbom).map_err(|_| broken("the SBOM is not UTF-8"))?;
    Ok(Downloaded {
        artefact: SignedArtefact {
            manifest: header.manifest,
            sbom,
            payload,
            signature: header.signature,
        },
        signer: header.signer,
    })
}

/// Download `id`@`version` and verify it against `anchors` — the caller's own trust in
/// release signers, not the registry's — before returning it. A signer the caller does
/// not anchor, or an artefact that does not verify, is an error. [FR-B-4]
pub fn download_verified(
    session: &mut Session,
    id: &str,
    version: &str,
    anchors: &attest::TrustAnchors,
) -> Result<Downloaded, CallError> {
    let got = download(session, id, version)?;
    let key = anchors.get(&got.signer).ok_or_else(|| {
        broken(format!(
            "signer {:?} is not one this caller trusts",
            got.signer
        ))
    })?;
    bifrost_artefact::verify(key, &got.artefact)
        .map_err(|e| broken(format!("{id}@{version} does not verify here: {e}")))?;
    Ok(got)
}

struct Header {
    manifest: ArtefactManifest,
    signer: String,
    signature: Signature,
}

/// Read one part (`sbom` or `payload`) slice by slice.
fn fetch_part(
    session: &mut Session,
    id: &str,
    version: &str,
    part: &str,
) -> Result<(Header, Vec<u8>), CallError> {
    let mut bytes = Vec::new();
    let mut header: Option<(Header, usize)> = None;
    loop {
        let reply = call(
            session,
            GET_ARTEFACT,
            &Json::obj(vec![
                ("id", Json::s(id)),
                ("version", Json::s(version)),
                ("part", Json::s(part)),
                ("offset", Json::Int(bytes.len() as i64)),
            ]),
        )?;
        let data = reply
            .str_field("data")
            .ok_or_else(|| broken("no data in the reply"))
            .and_then(|d| b64::decode(d).map_err(broken))?;
        if header.is_none() {
            header = Some(parse_header(&reply)?);
        }
        let size = header.as_ref().map_or(0, |h| h.1);
        if data.is_empty() && bytes.len() < size {
            return Err(broken("the registry sent an empty slice before the end"));
        }
        bytes.extend_from_slice(&data);
        if bytes.len() >= size {
            break;
        }
    }
    let (header, size) = header.expect("one reply was read");
    if bytes.len() != size {
        return Err(broken("the registry sent more bytes than the part holds"));
    }
    Ok((header, bytes))
}

fn parse_header(reply: &Json) -> Result<(Header, usize), CallError> {
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
        Header {
            manifest,
            signer: field(reply, "signer")?,
            signature: Signature::new(alg, signature),
        },
        size,
    ))
}
