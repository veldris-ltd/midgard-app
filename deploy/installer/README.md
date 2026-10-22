# Install Midgard on a server

`mg-install` puts the Midgard Suite, whole or in part, on one Linux server. It asks what you will use Midgard for, recommends the products and security settings for that use, enrols the first people, builds and starts everything, and then checks each part answers. It is written for someone who has not installed Midgard before. Afterwards the same command looks after the install: status, users, backups, logs and removal. [RC-33] [NFR-SEC-1]

```bash
git clone <the Midgard repository URL> midgard && cd midgard
sudo deploy/installer/mg-install
```

That is the whole install. The rest of this page explains each screen, the choices behind it and what to do afterwards.

> **Status.** Midgard has no tagged release yet. The installer builds the services from the source checkout you run it from, on the server itself. The images are not signed release images. That is suitable for evaluation, pilots and training; production use waits on the release and on the items in [GA_READINESS.md](../../docs/compliance/GA_READINESS.md). What each product does today, and how far, is in the [claims ledger](../../docs/compliance/CLAIMS.md).

---

## Contents

1. [What you need](#what-you-need)
2. [Choosing a server](#choosing-a-server)
3. [The installer, screen by screen](#the-installer-screen-by-screen)
4. [Use-case profiles](#use-case-profiles)
5. [Opening Midgard from your computer](#opening-midgard-from-your-computer)
6. [After the install](#after-the-install)
7. [Unattended installs](#unattended-installs)
8. [Servers running Plesk](#servers-running-plesk)
9. [Air-gapped sites and Kubernetes](#air-gapped-sites-and-kubernetes)
10. [What the installer changes on the server](#what-the-installer-changes-on-the-server)
11. [Security model](#security-model)
12. [Troubleshooting](#troubleshooting)

---

## What you need

- A Linux server you can reach over SSH, and an account on it that can use `sudo`.
- Ubuntu 22.04 or 24.04, Debian 12 or 13, or RHEL, Rocky Linux or AlmaLinux 9, on x86_64 or aarch64. Fedora also works. Other distributions are not tested but work if Docker does.
- At least 4 processor cores, 8 GB of memory and 20 GB of free disk. The [next section](#choosing-a-server) gives the sizes each use is designed for.
- Internet access from the server during the install, to Docker Hub, `static.rust-lang.org`, the npm registry (for the web console) and `huggingface.co` (for the AI model). Once installed, Midgard needs no network at all. A server that cannot reach Hugging Face can be given the model file instead: see [Model](#5-ai-model-and-access).
- About 20 to 60 minutes. The first build downloads 2 to 4 GB.
- A copy of this repository on the server (`git clone`, or copy the folder across).
- An authenticator app on the first user's phone or computer, such as Microsoft Authenticator, Google Authenticator or KeePassXC.

Check a server without changing anything:

```bash
deploy/installer/mg-install check
```

## Choosing a server

The installer measures the server and places it in a sizing tier. Each use-case profile names the tier it is designed for; a smaller server still runs, with less headroom, and the installer says so.

| Tier | Processor | Memory | Disk | Suits |
|---|---|---|---|---|
| **S**, evaluation | 4+ cores | 8+ GB (16 GB with the AI model) | 20 GB free, 100 GB recommended | trying Midgard, demonstrations, training |
| **M**, team server | 8+ cores | 32+ GB | 250 GB NVMe, mirrored | a team's real work: legal, healthcare, utilities |
| **G**, GPU server | 16+ cores | 128+ GB | 2 × 960 GB or larger NVMe, mirrored | defence, space, and AI-heavy work as GPU serving lands |

The disk figures come from the build: the toolchain and service images and their build cache take about 6 GB, and the Granite model about 3 GB while it is staged. Everything else is headroom for your data, backups and logs.

### Worked example: Hetzner GEX131

The GEX131 is a dedicated GPU server: an Intel Xeon Gold 5412U (24 cores, 48 threads), 256 GB of DDR5 ECC memory, two 960 GB Gen4 NVMe drives and an NVIDIA RTX PRO 6000 Blackwell Max-Q with 96 GB of GDDR7, on a 10 Gbit uplink ([Hetzner GEX131](https://www.hetzner.com/dedicated-rootserver/matrix-gpu/), [Hetzner GPU server documentation](https://docs.hetzner.com/robot/dedicated-server/server-lines/gpu-server/)). Check Hetzner's site for the current price and availability.

The installer recognises it and places it in tier G. To install on one:

1. In Hetzner Robot, install Ubuntu 24.04 with the drives in software RAID 1 (the `installimage` default for two drives), and add your SSH key.
2. Log in as root and update it: `apt-get update && apt-get -y full-upgrade && reboot`.
3. Create your own account, add it to `sudo`, and log in as it from now on.
4. Clone the repository and run `sudo deploy/installer/mg-install`. Answer **yes** when it offers to install Docker.
5. Pick the profile for your use. The defence and space profiles are designed for this tier.

**About the GPU.** The installer serves the AI model on the processor, through llama.cpp, which Midgard has run end to end. Serving on GPUs through vLLM is wired into Midgard's code and its Kubernetes chart but has not yet been run, so the installer does not offer it and leaves the GPU untouched. On a GEX131 the 24-core processor runs the Granite model comfortably in the meantime. The installer does not install NVIDIA drivers.

## The installer, screen by screen

Nothing on the server changes before you confirm screen 7. Press Ctrl+C at any point before then to stop.

### 1. Welcome

What will happen, how long it takes, and the status note above.

### 2. Server check

The measurements and a line for each check: architecture, distribution, sizing tier, free disk, Docker, ports 7710 and 5173, Plesk, a GEX131, a GPU, and internet access for the build. A red ✗ stops the install and says what to fix. If Docker is missing, the installer offers to install Docker Engine and the Compose plugin from Docker's own package repository (on Plesk servers it suggests Plesk's Docker extension first).

### 3. What will you use it for?

Pick the closest [profile](#use-case-profiles). The installer then shows the guidance for that use (regulation to consider, data handling, what Midgard does not do) and whether the server meets the size the profile is designed for.

### 4. Products

The core, Midgard itself, is always installed: identity, the object fabric, policy and the signed audit ledger. Turn the others on or off by typing their number.

| Product | What it does on this install |
|---|---|
| **Forge** | Runs scheduled jobs that read files, PostgreSQL tables, S3-compatible object stores or Kafka topics into versioned datasets, and syncs them into the core. The profile's jobs read its sample files. |
| **Mimir** | Serves AI inference on this server, to certified callers over post-quantum mutual TLS. Every call is authorised and written to its own signed ledger. Today the console does not call it; `mimir-probe` does. |
| **Bifrost** | The fleet manager: installations check in with the version they run, and operators see drift from each release channel. It records; it does not push upgrades. |
| **Heimdall** | The feed gateway: sources submit signed observations, which are fused into one operational picture. |

### 5. AI model and access

If Mimir is installed, choose its model:

- **Granite 3.3 2B Instruct** (IBM, Apache-2.0), run on the processor. The installer downloads it from a pinned revision, refuses it unless its SHA-256 is the one IBM publishes, signs a manifest for it and verifies it the way Mimir does. Mimir checks the signature and the weights again before every start. If the server cannot reach `huggingface.co`, the installer asks for the path of the file instead: download `granite-3.3-2b-instruct-Q4_K_M.gguf` elsewhere ([the pinned file](https://huggingface.co/ibm-granite/granite-3.3-2b-instruct-GGUF/resolve/7cdf86ccd1f1bb3491c9b7017b033f2e51367397/granite-3.3-2b-instruct-Q4_K_M.gguf)) and copy it to the server.
- **Reference engine**: fixed answers and no model. Use it to test the plumbing, never for real questions.

Then choose whether to install the **web console**. It is built on the server and served on the loopback address next to the gateway.

### 6. People

- **The first user** gets the role `operator` (read, and run actions) with the profile's clearance and purposes. Their password is asked for at the install step, used once and never stored. Regulated profiles require a second factor: an authenticator app code at every sign-in.
- **Two administrators**, for regulated profiles. Enrolling anyone new while Midgard runs then needs dual control: one administrator requests, a different one approves. Each administrator gets a post-quantum passkey file (ML-DSA-65).

### 7. Review

A summary of every choice. Confirm, and the install starts.

### 8. Install

Each step shows one line and a tick. The detail goes to `/opt/midgard/install.log`.

1. Copy the source to `/opt/midgard/src`.
2. Build the pinned Rust toolchain image (first time only), then the Midgard services, offline against the vendored dependencies.
3. Build the web console.
4. Fetch, verify and sign the model.
5. Create the enclosure: its issuer and audit keys, and a mesh identity and certificate for each service.
6. Enrol the first user. **If they have a second factor, the screen shows a QR code (when `qrencode` is installed) and the key, once. Scan it then: it is not saved anywhere.**
7. Create the administrators.
8. Start the services and wait until each reports ready.
9. Check the result: the gateway answers; Forge's first sync lands; the first user signs in (the installer computes their authenticator code itself, in memory) and can read the synced data; Mimir answers an inference call over PQ-mTLS; Bifrost and Heimdall answer reads; the console is served.

If a step fails, the screen shows the end of the log. Fix the cause and run the installer again: finished steps are skipped, so it resumes where it stopped.

## Use-case profiles

A profile sets the starting point. The products and the model can be changed on screens 4 and 5.

| Profile | Products | Model | Second factor | Administrators | First user's clearance | Purposes | Designed for |
|---|---|---|---|---|---|---|---|
| Evaluation and demonstration | all four | reference | recommended | none | internal | operations | tier S |
| Legal and professional services | Forge, Mimir | Granite | required | 2 | restricted | operations, case-work | tier M |
| Healthcare and life sciences | Forge, Mimir | Granite | required | 2 | confidential | operations, direct-care | tier M |
| Defence and security | all four | Granite | required | 2 | restricted | operations | tier G |
| Space: mission and ground segment | all four | Granite | required | 2 | restricted | operations, mission-ops | tier G |
| Utilities and critical infrastructure | Forge, Mimir, Heimdall | Granite | required | 2 | restricted | operations, asset-management | tier M |

Every sector profile brings a **starter schema** (object types, links and human-gated actions), a policy that lets the `operator` role run those actions and the `viewer` role read, and Forge jobs with a few rows of clearly fictional sample data, so the first sign-in shows something real. The files are in [`profiles/`](profiles/); replace the sample data with your own as described in [Load your own data](#load-your-own-data).

| Profile | Starter schema | Human-gated actions |
|---|---|---|
| Evaluation | aircraft, facilities (the demonstration schema in `deploy/compose`) | none |
| Legal | matter, party, document | place legal hold; privilege review (unreviewed, privileged, disclosable) |
| Healthcare | pseudonymised patient, care unit, encounter | request clinical review |
| Defence | unit, site, asset | set readiness (green, amber, red); declare unserviceable |
| Space | spacecraft, ground station, contact window, anomaly | advance anomaly triage; record safe-mode decision |
| Utilities | substation, feeder, transformer (the schema from the [import guide](../../docs/guides/IMPORT_YOUR_OWN_DATA.md)) | flag for inspection; isolate substation |

**Why every profile includes the purpose `operations`.** The gateway signs every session in for the purpose `operations` today (`LOGIN_PURPOSE` in `services/http-gateway/src/lib.rs`). A user without that purpose cannot sign in, and data without it cannot be read in a session. The sector purposes (`case-work`, `direct-care` and so on) are recorded on the data alongside it, ready for when sessions can declare their own purpose. `mg-install add-user` adds `operations` if it is left out.

### Guidance by sector

The installer shows this guidance on screen 3 and writes it into the install report. It is a starting point for your own assessment, not legal or regulatory advice.

**Legal.** Client material stays on the server, and Mimir runs the model locally. Choose a hosting region that matches your data residency commitments (UK GDPR, and client or panel terms). Treat privileged material as restricted and give each user only the clearance their matters need. Agree a retention schedule before loading live matters: imports and syncs only add, they never delete.

**Healthcare.** Health data is special category data under UK GDPR: complete a DPIA before loading real records. Load pseudonymised data only; the starter schema has no name, address or NHS number fields by design. Midgard is not a medical device and has no clinical safety case, so its output is not for clinical decisions. NHS organisations in England should also consider the Data Security and Protection Toolkit and DCB0129 / DCB0160.

**Defence.** This build is not accredited for classified material. Midgard's marking bands (open, internal, restricted, confidential) are not government classification markings. Install on hardware you control, then disconnect it: Midgard needs no network once installed. Rented and cloud servers suit exercises and training data, not operational data.

**Space.** Midgard records decisions about spacecraft; it does not command them and has no link to a mission control system. Check export control (UK Strategic Export Controls, and ITAR or EAR for US-origin data) before loading mission data. Telemetry summaries enter through Heimdall feed sources or Forge jobs; raw telemetry processing is out of scope.

**Utilities.** Isolating a substation in Midgard records a decision; it does not operate plant and has no link to SCADA. Consider the NIS Regulations 2018 and the NCSC Cyber Assessment Framework, and keep the server on a network segment separate from operational technology.

## Opening Midgard from your computer

Midgard listens only on the server's loopback address: the gateway on `127.0.0.1:7710` and the console on `127.0.0.1:5173`. Nothing is reachable from the internet. You reach both through an SSH tunnel, which encrypts the connection and needs no extra software on Windows 10 and 11, macOS or Linux.

On your own computer, in a terminal (PowerShell on Windows), run this and leave it running:

```bash
ssh -N -L 5173:127.0.0.1:5173 -L 7710:127.0.0.1:7710 you@your-server
```

Then open **http://localhost:5173** in your browser and sign in with the first user's name, password and authenticator code. The installer prints this command with your server's name filled in, and writes it into the install report.

Both ports must be forwarded: the console runs in your browser and talks to the gateway at `127.0.0.1:7710`. The gateway accepts the console only from `http://localhost:5173` and `http://127.0.0.1:5173`.

## After the install

The installer links itself as `/usr/local/bin/mg-install`, so from any folder:

| Task | Command |
|---|---|
| What is running, and is it healthy | `sudo mg-install status` |
| Start, stop, restart every service | `sudo mg-install start`, `stop`, `restart` |
| Follow a service's log | `sudo mg-install logs gateway` (or `mimir-server`, `forge-worker`, `bifrost-fleet`, `heimdall-feed`, `console`) |
| Enrol someone | `sudo mg-install add-user` |
| Back up every volume | `sudo mg-install backup` |
| Delete the administrators' passkeys once handed over | `sudo mg-install handed-over` |
| Check, repair, or change the choices | `sudo mg-install` |
| Remove the services, keep the data | `sudo mg-install uninstall` |
| Remove everything, data included | `sudo mg-install uninstall --purge` |

`/opt/midgard/INSTALL_REPORT.md` records what was installed, how to reach it and the guidance for your use. It holds no secrets.

### Hand over the administrators' passkeys

A passkey file is a bearer secret: whoever holds it can act as that administrator. After the install, give each administrator their file from `/opt/midgard/admin-passkeys/` over a channel you trust (in person, on encrypted media, or as an attachment in their encrypted password manager), never by email or chat. Then delete the server's copies:

```bash
sudo mg-install handed-over
```

### Add people

`sudo mg-install add-user` asks for the name, role (`operator` reads and runs actions, `viewer` only reads), clearance, purposes, second factor and password, then enrols them. The gateway stops for a few seconds while it does. With administrators set up, people can instead be enrolled while Midgard runs, under dual control: see [Administrators and online enrolment](../../README.md#administrators-and-online-enrolment).

### Load your own data

Each sector profile's Forge jobs read the CSV files in `/opt/midgard/site/forge/`. To load your own:

1. Edit or replace the CSV files, keeping the columns the jobs name. Forge syncs changes on each job's interval (60 to 600 seconds).
2. To read from PostgreSQL, S3-compatible stores or Kafka instead, change the job's `source`. The format is documented at the top of `forge/worker/src/lib.rs`.
3. To change the schema, edit the profile's `ontology.json` and `policy.json`. A running enclosure keeps the schema it was created with, so a changed schema needs a fresh enclosure, or the offline route in [Import your own data](../../docs/guides/IMPORT_YOUR_OWN_DATA.md).

Re-running `sudo mg-install` rewrites `/opt/midgard/site` from the profile, so keep your own files somewhere else too, or edit the profile in `/opt/midgard/src/deploy/installer/profiles/`.

### Back up and restore

```bash
sudo mg-install backup                 # to /var/backups/midgard/<timestamp>/
sudo mg-install backup /mnt/backups    # or anywhere else
```

The services stop while each Docker volume is archived, then start again. The folder gets one `.tar.gz` per volume, a `SHA256SUMS` file and a copy of `install.env`. The stores are encrypted at rest, but the `core-state` archive holds the enclosure's sealed keys: protect the backup as you would the server.

<a id="restore"></a>To restore onto a fresh install with the same choices:

```bash
cd /var/backups/midgard/<timestamp> && sha256sum -c SHA256SUMS
sudo mg-install stop
for f in *.tar.gz; do
  v="${f%.tar.gz}"
  docker volume create "$v" >/dev/null
  docker run --rm -u 0 --network none -v "$v:/v" -v "$PWD:/b:ro" midgard/dev:local \
    sh -c "find /v -mindepth 1 -delete && tar -C /v -xzf /b/$f"
done
sudo mg-install start
```

Every store re-verifies its signed chain when it opens, so a damaged or altered archive is refused at start rather than served. The design behind this is in [BACKUP_RESTORE.md](../../docs/ops/BACKUP_RESTORE.md).

### Update

Get the newer source on the server, then run its installer. It copies the new source over `/opt/midgard/src`, rebuilds, and restarts with the same choices and the same data:

```bash
cd ~/midgard && git pull && sudo deploy/installer/mg-install
```

Choose **Check and repair it, with the same choices**.

## Unattended installs

For repeatable installs, or provisioning from a script, put the answers in a file and pass it:

```bash
install -m 0600 /dev/null /root/operator.pw && nano /root/operator.pw   # the first user's password
sudo deploy/installer/mg-install --answers /root/midgard.answers
shred -u /root/operator.pw
```

[`answers.example`](answers.example) lists every key. The file is read as `KEY=VALUE` lines and never executed. An unattended install changes nothing unless the file says `CONFIRM=yes`. It prints the first user's authenticator key once, in its output: if you capture that output to a file, treat the file as a secret and delete it once the key is in the app.

## Servers running Plesk

The installer detects Plesk and leaves it alone. Midgard listens only on `127.0.0.1`, so it does not touch Plesk's web server or ports 80 and 443. On a Plesk server:

- Install Docker through Plesk (Extensions, Docker) rather than letting the installer add Docker's repository, so Plesk keeps managing it.
- Reach Midgard through the SSH tunnel as above. If you later publish it on a domain, do it through that domain's Apache and nginx settings in Plesk with a TLS certificate, never by hand-editing Apache's configuration. The gateway warns that anything beyond loopback must sit behind TLS termination.
- Plesk servers are often shared. The evaluation profile with the reference engine is the lightest install; Granite needs about 4 GB of memory more.

## Air-gapped sites and Kubernetes

The installer is for one server with internet access during the install. Two other pathways exist, at different stages:

- **Air-gapped sites.** Midgard needs no network once installed, so the simplest route today is to install on a connected server and then disconnect it. The intended route is a signed offline bundle produced on the connected side and verified offline before anything is deployed ([air-gap bundle](../air-gap-bundle/README.md)). That route needs the signed release images, which are not built until the first release is tagged. Models cross the gap as their own signed bundles (`mg-model pack` and `unpack`).
- **Kubernetes.** The Helm chart in [`deploy/helm/midgard`](../helm/midgard) deploys the same services, hardened and default-deny, and has been run on a local three-node kind cluster (`just kind-up`, see [the README](../../README.md#run-on-kubernetes-with-kind)). Its release values expect signed per-service images, which are not built yet, so a production cluster install also waits on the first release.

## What the installer changes on the server

So you know exactly what it did, and what to remove by hand if you ever need to:

| What | Where |
|---|---|
| Docker Engine and the Compose plugin, and Docker's package repository | only if you said yes on screen 2 |
| The source, the site configuration, the log, the report, `install.env` (choices only, no secrets) | `/opt/midgard` (change it with `MG_INSTALL_DIR`) |
| Administrator passkeys, until handed over | `/opt/midgard/admin-passkeys`, mode 0700 |
| The command `mg-install` | a link at `/usr/local/bin/mg-install` |
| Images | `midgard-acceptance:1.82.0`, `midgard/dev:local`, `midgard/console:local`, and the pinned llama.cpp image with Granite |
| Volumes | `midgard_*` (one per service's state and identity), `midgard-models`, `midgard-model-signing` |
| Containers and networks | the `midgard` Compose project; networks `midgard_edge` and `midgard_mesh` (internal) |
| Listening ports | `127.0.0.1:7710` (gateway) and `127.0.0.1:5173` (console) only |

It changes no firewall rule, no web server, no system service other than Docker, and no user account.

## Security model

What the install protects, and what it does not yet:

- **Nothing is exposed.** Only loopback ports are published. The services talk to one another on an internal Docker network with no route out, over post-quantum mutual TLS, each with its own certificate from the enclosure's issuer.
- **Hardened containers.** Every Midgard container runs as a non-root user, with a read-only root filesystem, no Linux capabilities and `no-new-privileges`, and mounts only its own volumes. No service sees the enclosure's keys except the gateway's state.
- **Encrypted, signed state.** Stores are sealed at rest (AES-256-GCM under ML-KEM-1024-wrapped keys) and every audit ledger is signed with ML-DSA and re-verified at start.
- **Secrets stay out of files.** Passwords are typed when used and never stored or logged; the authenticator key is shown once; the install log and report hold no secrets.
- **Verified model.** The model is checked against the upstream SHA-256, signed into a manifest and re-verified by Mimir before every start.

Not yet, and why it matters:

- **The images are built from source and unsigned.** Signed, verify-before-run release images arrive with the first tagged release.
- **Key custody is in software.** The enclosure's keys are sealed files in a Docker volume, not in an HSM. Protect the server, and its backups, accordingly.
- **The model-signing key is a development key** kept on the same server. In production it stays in a separate staging enclave.
- **Certificates do not expire.** To rotate a service's identity, remove its identity files from its volume and re-run the installer.
- **The tunnel is the transport security for the console.** The gateway speaks plain HTTP on loopback; the SSH tunnel encrypts it between your computer and the server.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| A step fails | The screen shows the end of `/opt/midgard/install.log`. Fix the cause and run `sudo mg-install` again: it resumes. |
| "Cannot reach Docker Hub or static.rust-lang.org" | The build needs them once. Check the server's outbound firewall and DNS, or any proxy. |
| "Port 7710 (or 5173) is already in use" | Another program holds it. Find it with `sudo ss -ltnp 'sport = :7710'`. |
| Building the services fails with "No space left on device" | Free disk where Docker keeps images (`docker system df`, `docker builder prune`). |
| Building the console fails | The npm registry is unreachable. Re-run and answer **no** to the web console; the gateway's API works without it. |
| `mimir-server` never becomes ready with Granite | Usually memory: Granite needs about 4 GB more. Check `sudo mg-install logs mimir-llamacpp`, or reinstall with the reference engine. |
| The console loads but sign-in does nothing | Both ports must be forwarded, and the browser must use `http://localhost:5173` or `http://127.0.0.1:5173` exactly. |
| The authenticator code is refused | The server's and phone's clocks must agree within 30 seconds. Check the server with `timedatectl`. |
| Signed out after an hour | Expected: a session lasts an hour, and survives restarts. Sign in again. |
| Forget the first user's authenticator | Enrol a new user with `sudo mg-install add-user`. An enrolled user is never replaced. |
