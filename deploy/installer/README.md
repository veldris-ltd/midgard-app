# Install Midgard on a server

`mg-install` puts the Midgard Suite, whole or in part, on one Linux server. It asks what you will use Midgard for (legal practice, healthcare, military or utilities), recommends the products and security settings for that use, loads sample data and, if you want them, recorded Heimdall feeds, enrols the first people, builds and starts everything, and then checks each part answers. It is written for someone who has not installed Midgard before. Afterwards the same command looks after the install: status, users, backups, logs and removal. [RC-33] [NFR-SEC-1]

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
5. [Sample data and Heimdall feeds](#sample-data-and-heimdall-feeds)
6. [Opening Midgard from your computer](#opening-midgard-from-your-computer)
7. [After the install](#after-the-install)
8. [Unattended installs](#unattended-installs)
9. [Servers running Plesk](#servers-running-plesk)
10. [Air-gapped sites and Kubernetes](#air-gapped-sites-and-kubernetes)
11. [What the installer changes on the server](#what-the-installer-changes-on-the-server)
12. [Security model](#security-model)
13. [Troubleshooting](#troubleshooting)

---

## What you need

- A Linux server you can reach over SSH, and an account on it that can use `sudo`.
- Ubuntu 22.04 or 24.04, Debian 12 or 13, or RHEL, Rocky Linux or AlmaLinux 9, on x86_64 or aarch64. Fedora also works. Other distributions are not tested but work if Docker does.
- At least 4 processor cores, 8 GB of memory and 20 GB of free disk. The [next section](#choosing-a-server) gives the sizes each use is designed for.
- Internet access from the server during the install, to Docker Hub, `static.rust-lang.org`, the npm registry (for the web console) and `huggingface.co` (for the AI model). Once installed, Midgard needs no network at all. A server that cannot reach Hugging Face can be given the model file instead: see [Model](#5-ai-model-and-access).
- Python 3.8 or later, which every supported distribution packages. The installer offers to install it if it is missing.
- About 20 to 60 minutes. The first build downloads 2 to 4 GB.
- Optionally, a copy of `midgard-sample-datasets.zip` on the server, for the recorded data and feeds that are not in this repository (see [Sample data and Heimdall feeds](#sample-data-and-heimdall-feeds)).
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
| **G**, GPU server | 16+ cores | 128+ GB | 2 × 960 GB or larger NVMe, mirrored | military, and AI-heavy work as GPU serving lands |

The disk figures come from the build: the toolchain and service images and their build cache take about 6 GB, and the Granite model about 3 GB while it is staged. Everything else is headroom for your data, backups and logs.

### Worked example: Hetzner GEX131

The GEX131 is a dedicated GPU server: an Intel Xeon Gold 5412U (24 cores, 48 threads), 256 GB of DDR5 ECC memory, two 960 GB Gen4 NVMe drives and an NVIDIA RTX PRO 6000 Blackwell Max-Q with 96 GB of GDDR7, on a 10 Gbit uplink ([Hetzner GEX131](https://www.hetzner.com/dedicated-rootserver/matrix-gpu/), [Hetzner GPU server documentation](https://docs.hetzner.com/robot/dedicated-server/server-lines/gpu-server/)). Check Hetzner's site for the current price and availability.

The installer recognises it and places it in tier G. To install on one:

1. In Hetzner Robot, install Ubuntu 24.04 with the drives in software RAID 1 (the `installimage` default for two drives), and add your SSH key.
2. Log in as root and update it: `apt-get update && apt-get -y full-upgrade && reboot`.
3. Create your own account, add it to `sudo`, and log in as it from now on.
4. Clone the repository and run `sudo deploy/installer/mg-install`. Answer **yes** when it offers to install Docker.
5. Pick the profile for your use. The military profile is designed for this tier; the others run on it with room to spare.

**About the GPU.** The installer serves the AI model on the processor, through llama.cpp, which Midgard has run end to end. Serving on GPUs through vLLM is wired into Midgard's code and its Kubernetes chart but has not yet been run, so the installer does not offer it and leaves the GPU untouched. On a GEX131 the 24-core processor runs the Granite model comfortably in the meantime. The installer does not install NVIDIA drivers.

## The installer, screen by screen

Nothing on the server changes before you confirm screen 8. Press Ctrl+C at any point before then to stop.

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

### 6. Data and feeds

Forge always loads the profile's own fictional records. The military and utilities profiles also load recorded datasets from the [Midgard sample datasets](../../samples/README.md). Give the path of `midgard-sample-datasets.zip` to use the full set, or press Enter for the openly licensed part in this repository.

If Heimdall is installed, choose the feed families to replay into its operating picture. Each family shows how many of its feeds are available from the source you chose; see [Sample data and Heimdall feeds](#sample-data-and-heimdall-feeds).

### 7. People

- **The first user** gets the role `operator` (read, and run actions) with the profile's clearance and purposes. Their password is asked for at the install step, used once and never stored. Regulated profiles require a second factor: an authenticator app code at every sign-in.
- **Two administrators**, for regulated profiles. Enrolling anyone new while Midgard runs then needs dual control: one administrator requests, a different one approves. Each administrator gets a post-quantum passkey file (ML-DSA-65).

### 8. Review

A summary of every choice. Confirm, and the install starts.

### 9. Install

Each step shows one line and a tick. The detail goes to `/opt/midgard/install.log`.

1. Copy the source to `/opt/midgard/src`, and unpack the sample datasets zip if you gave one.
2. Build the pinned Rust toolchain image (first time only), then the Midgard services, offline against the vendored dependencies.
3. Write this server's configuration: the profile's schema, policy and Forge jobs, joined with the sample configuration's where the profile loads recorded datasets.
4. Build the web console.
5. Fetch, verify and sign the model.
6. Create the enclosure: its issuer and audit keys, and a mesh identity and certificate for each service.
7. Enrol the first user. **If they have a second factor, the screen shows a QR code (when `qrencode` is installed) and the key, once. Scan it then: it is not saved anywhere.**
8. Create the administrators, and issue each chosen Heimdall feed its own mesh identity.
9. Start the services and wait until each reports ready.
10. Check the result: the gateway answers; **every** Forge job has synced (a failed job stops the install and is named); the first user signs in (the installer computes their authenticator code itself, in memory) and can read the synced data; Mimir answers an inference call over PQ-mTLS; Bifrost and Heimdall answer reads; the console is served.
11. Replay the chosen feeds into Heimdall, each observation signed by its feed's identity, and read the picture back.

If a step fails, the screen shows the end of the log. Fix the cause and run the installer again: finished steps are skipped, so it resumes where it stopped.

## Use-case profiles

A profile sets the starting point. The products, model and feeds can be changed on screens 4 to 6.

| Profile | Products | Recorded datasets | Default feeds | Clearance and purposes of the first user | Designed for |
|---|---|---|---|---|---|
| **Legal practice (European Union)** | Forge, Mimir | none | none | restricted; operations, case-work, compliance | tier M |
| **Healthcare (NHS aligned)** | Forge, Mimir | none | none | confidential; operations, direct-care | tier M |
| **Military (British and EU aligned)** | Forge, Mimir, Bifrost, Heimdall | weather, earthquakes, space weather; aircraft (full set) | aviation, maritime, communications, hazards, information | restricted; operations | tier G |
| **Utilities (water, electricity, transport, communications)** | Forge, Mimir, Heimdall | GB grid frequency, fuel mix and carbon intensity, floods, TfL, weather, earthquakes, space weather; air quality (full set) | energy, water, transport, communications, hazards | restricted; operations, asset-management | tier M |

Every profile uses the Granite model, requires an authenticator code at sign-in, and creates two administrators for dual control. Each brings a **starter schema**, a policy that lets the `operator` role run its actions and the `viewer` role read, and Forge jobs over clearly fictional records, so the first sign-in shows something real. The files are in [`profiles/`](profiles/).

| Profile | Starter schema | Human-gated actions |
|---|---|---|
| Legal | client, matter, party, conflict check, document, deadline, time entry, AML check, data subject request | place or lift a legal hold; close a matter; privilege review (unreviewed, privileged, disclosable, redact and disclose); record a conflict result; mark a deadline met; approve time; escalate an AML check; advance a data subject request |
| Healthcare | NHS organisation (ODS code), care unit (treatment function code), pseudonymised patient, encounter (SNOMED CT), referral (RTT clock), clinical safety hazard (DCB0129 / DCB0160), Caldicott review | request clinical review; change referral priority (routine, urgent, urgent suspected cancer); stop the RTT clock; record hazard mitigation; record a Caldicott Guardian decision |
| Military | formation, unit, site, asset, tasking, incident; with the sample configuration's airports, aircraft, weather, Heimdall tracks and alerts | set readiness (green, amber, red); declare unserviceable; return to service; advance a tasking; escalate an incident; and the sample configuration's 17 aviation, weather and alert actions |
| Utilities | substation, feeder, transformer, water asset, telecommunications site, transport asset, each linked to its region; with the sample configuration's grid, flood, TfL, weather and alert types | flag a transformer or water asset for inspection; isolate or re-energise a substation; set a telecommunications site or transport asset's status; and the sample configuration's actions |

**Why every profile includes the purpose `operations`.** The gateway signs every session in for the purpose `operations` today (`LOGIN_PURPOSE` in `services/http-gateway/src/lib.rs`). A user without that purpose cannot sign in, and data without it cannot be read in a session. The sector purposes (`case-work`, `direct-care` and so on) are recorded on the data alongside it, ready for when sessions can declare their own purpose. `mg-install add-user` adds `operations` if it is left out.

### Guidance by sector

The installer shows this guidance on screen 3 and writes it into the install report. It is a starting point for your own assessment, not legal or regulatory advice.

**Legal practice (European Union).** Host in the EU (for Hetzner: Falkenstein or Nuremberg in Germany, or Helsinki in Finland); GDPR (Regulation (EU) 2016/679) governs client personal data. Professional secrecy and privilege differ by Member State, and the CCBE Code of Conduct for European Lawyers is the common baseline. Conflict checks, AML checks and data subject requests are records with their own actions: the AML Regulation (EU) 2024/1624 applies from 10 July 2027, and GDPR Article 12(3) gives one month to answer a data subject request. Mimir runs the model on the server; before relying on its output in client work, consider the AI Act (Regulation (EU) 2024/1689) and your bar's guidance. Imports and syncs only add, never delete, so agree a retention schedule first; each matter carries a `retention_until` date.

**Healthcare (NHS aligned).** Health data is special category data (UK GDPR Article 9 and the Data Protection Act 2018): complete a DPIA and include the system in your Data Security and Protection Toolkit submission before live use. Load pseudonymised data only; the schema has no name, address or NHS number fields by design. DCB0129 and DCB0160 apply to health IT, and the hazard log is there for your clinical safety officer. Midgard is not a medical device and has no clinical safety case: its output is not for clinical decisions. Record Caldicott Guardian decisions on new uses of data; the national data opt-out applies to uses beyond individual care. Codes follow NHS standards (ODS, treatment function codes, SNOMED CT); keep records for the periods in the NHS Records Management Code of Practice.

**Military (British and EU aligned).** This build is not accredited: do not process UK classified information above OFFICIAL, or NATO or EU classified information, on it. Midgard's bands (open, internal, restricted, confidential) are not UK Government Security Classifications, NATO or EU markings, and any mapping between them is for your accreditor to set. UK use follows JSP 440 and your accreditation authority; the security rules for EU classified information are in Council Decision 2013/488/EU. Install on hardware you control, then disconnect it; rented and cloud servers suit exercise and training data only. The Heimdall feeds are open-source information for exercises, not an intelligence source.

**Utilities.** The recorded datasets are real GB grid, flood, TfL, weather, earthquake and space weather data from October 2026; the asset records are fictional. Isolating a substation or closing a road in Midgard records a decision; it does not operate plant and has no link to SCADA or other control systems. Operators of essential services should consider the NIS Regulations 2018 and the NCSC Cyber Assessment Framework, and in the EU the NIS2 Directive (EU) 2022/2555 and the CER Directive (EU) 2022/2557. Keep the server on a network segment separate from operational technology.

## Sample data and Heimdall feeds

The [Midgard sample datasets](../../samples/README.md) are real data recorded from live public sources on 22 October 2026. The installer uses two parts of them:

- **Recorded datasets** (Forge): the military and utilities profiles load them through Forge, beside the profile's fictional records, using the configuration they were recorded with.
- **Heimdall observations** (feeds): each chosen feed's recording is replayed into Heimdall, signed by that feed's own certified identity, so the operating picture fills with real tracks, notices and readings.

| Family | Feeds in this repository | Added by the full set |
|---|---|---|
| aviation | EASA conflict zone bulletins, FAA restrictions, airport METAR and TAF | ADS-B aircraft, ADS-B military aircraft, openAIP airspace |
| maritime | Baltic ship positions (Digitraffic) | AISStream ship positions |
| energy | GB grid status | power cuts, power and gas infrastructure, GB gas flows, European gas storage (GIE) |
| water | Environment Agency reservoir levels | storm overflows, water infrastructure |
| transport | Network Rail train movements | National Highways closures |
| communications | OONI internet censorship | telecommunications infrastructure, Cloudflare Radar outages |
| hazards | NASA FIRMS fire hotspots | |
| information | GDELT news volume | Gulf watch |

**Where the data comes from.** This repository is public, so it holds only files whose sources state an open licence that allows redistribution. The rest are in `midgard-sample-datasets.zip`; give its path on screen 6 to use them. [NOTICE.md](../../samples/NOTICE.md) lists every file, its licence and the credit to keep, and the install report repeats the credits for what was loaded. Files from the zip that are not cleared for redistribution are marked as such; Cloudflare Radar and openAIP data are licensed for non-commercial use only.

**How much is replayed.** Heimdall returns its whole picture to an analyst in one post-quantum TLS frame of at most 1 MiB. The installer therefore keeps the replayed picture inside a budget: small feeds are replayed whole and the largest are trimmed evenly, and the log names every trim. Measured: the full military set replays 1,457 entities from 14 feeds into a 701 KiB picture; the full utilities set, 1,406 entities from 15 feeds into 703 KiB.

**What a feed is not.** A replay is a recording: the picture shows the moment it was recorded and does not update. Connecting live sources is a separate step, with your own feed sources signing observations as `heimdall-source` does.

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

Each profile's Forge jobs read the CSV files in `/opt/midgard/site/forge/`, with the recorded datasets converted to CSV beside the profile's own. To load your own:

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

For repeatable installs, or provisioning from a script, put the answers in a file and pass it. `SAMPLES_ZIP` and `FEEDS` choose the data and feeds:

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
- Plesk servers are often shared. The reference engine is the lightest choice for Mimir; Granite needs about 4 GB of memory more. Leaving Heimdall out avoids the feed replay.

## Air-gapped sites and Kubernetes

The installer is for one server with internet access during the install. Two other pathways exist, at different stages:

- **Air-gapped sites.** Midgard needs no network once installed, so the simplest route today is to install on a connected server and then disconnect it. The intended route is a signed offline bundle produced on the connected side and verified offline before anything is deployed ([air-gap bundle](../air-gap-bundle/README.md)). That route needs the signed release images, which are not built until the first release is tagged. Models cross the gap as their own signed bundles (`mg-model pack` and `unpack`).
- **Kubernetes.** The Helm chart in [`deploy/helm/midgard`](../helm/midgard) deploys the same services, hardened and default-deny, and has been run on a local three-node kind cluster (`just kind-up`, see [the README](../../README.md#run-on-kubernetes-with-kind)). Its release values expect signed per-service images, which are not built yet, so a production cluster install also waits on the first release.

## What the installer changes on the server

So you know exactly what it did, and what to remove by hand if you ever need to:

| What | Where |
|---|---|
| Docker Engine and the Compose plugin, and Docker's package repository; Python 3 from the distribution's packages | only if you said yes on screen 2 |
| The full sample datasets, unpacked | `/opt/midgard/samples-full`, only if you gave the zip |
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
| "Python 3.8 or later is not installed" | Answer yes when the installer offers it, or install `python3` with the distribution's package manager. |
| "These Forge jobs did not sync" | The step names each job and the reason, such as a value of the wrong type in a CSV file. Fix the file in `/opt/midgard/site/forge/` or the profile, and run the installer again. |
| A feed replay refuses observations | The step prints the first refusals. A refusal usually means the feed has no identity: run the installer again, which issues any that are missing. |
| "The picture could not be read back" | The picture outgrew the 1 MiB frame, usually because feeds were replayed by hand on top of the installer's. Choose fewer feed families, or start a fresh enclosure. |
| The console loads but sign-in does nothing | Both ports must be forwarded, and the browser must use `http://localhost:5173` or `http://127.0.0.1:5173` exactly. |
| The authenticator code is refused | The server's and phone's clocks must agree within 30 seconds. Check the server with `timedatectl`. |
| Signed out after an hour | Expected: a session lasts an hour, and survives restarts. Sign in again. |
| Forget the first user's authenticator | Enrol a new user with `sudo mg-install add-user`. An enrolled user is never replaced. |
