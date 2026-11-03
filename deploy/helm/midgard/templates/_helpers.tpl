{{- /*
Secret custody: where a service's identity, keystore passphrase and seed come from.
[NFR-SEC-1] [VLD-ANB-MIDGARD-0018]
*/ -}}

{{- /*
midgard.validateSecrets: fail the render for a provider that does not exist, for the
kubernetes provider outside the kind profile without the etcd-encryption
acknowledgement, and for externalSecrets without a store. Renders nothing.
*/ -}}
{{- define "midgard.validateSecrets" -}}
{{- $s := .Values.secrets -}}
{{- if not (has $s.provider (list "kubernetes" "csi" "externalSecrets")) -}}
{{- fail (printf "secrets.provider must be kubernetes, csi or externalSecrets, not %q" $s.provider) -}}
{{- end -}}
{{- if and (eq $s.provider "kubernetes") (not $s.acknowledgeEtcdEncryption) (not .Values.image.single) -}}
{{- fail "secrets.provider=kubernetes needs etcd encryption at rest (EncryptionConfiguration with a KMS v2 provider); set secrets.acknowledgeEtcdEncryption=true once configured, or use csi or externalSecrets" -}}
{{- end -}}
{{- if and (eq $s.provider "externalSecrets") (not $s.externalSecrets.storeRef.name) -}}
{{- fail "secrets.provider=externalSecrets needs secrets.externalSecrets.storeRef.name" -}}
{{- end -}}
{{- end -}}

{{- /*
midgard.secretSource (list $ NAME): a pod volume source for the secret material NAME,
from the configured provider. kubernetes and externalSecrets both mount Secret NAME
(externalSecrets populates it); csi mounts the SecretProviderClass NAME.
*/ -}}
{{- define "midgard.secretSource" -}}
{{- $root := index . 0 -}}
{{- $name := index . 1 -}}
{{- if eq $root.Values.secrets.provider "csi" -}}
csi: { driver: secrets-store.csi.k8s.io, readOnly: true, volumeAttributes: { secretProviderClass: {{ $name }} } }
{{- else -}}
secret: { secretName: {{ $name }}, defaultMode: 288 }
{{- end -}}
{{- end -}}
