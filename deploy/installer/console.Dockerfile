# The Midgard console as a static site, for a server installed with mg-install.
# [RC-37] [NFR-SEC-1]
#
# Build context: the repository root.
#   docker build -f deploy/installer/console.Dockerfile -t midgard/console:local .
#
# The build stage installs the console's JavaScript dependencies from the pinned lock
# file (this needs the npm registry) and builds it against the gateway edge at
# http://127.0.0.1:7710, the address it has on the operator's machine through an SSH
# tunnel. The runtime stage serves the built files as a non-root user, read-only,
# and holds no secret: the console logs in to the gateway from the browser.
#
# Like the compose image, this is built from the checkout and is not a signed release.

ARG BUILD=node:22-bookworm-slim@sha256:de170b39439eb0e5a3444ad7d60b015614a381e10fecae2e30ec9e735122f38d
ARG RUNTIME=nginxinc/nginx-unprivileged:stable-alpine@sha256:f5116ed4606f3eb015dd4a8e28cb9b351476dc0ad4bc6e9f93b19cec130cdf03

FROM ${BUILD} AS build
WORKDIR /src
RUN corepack enable && corepack prepare pnpm@9.12.0 --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml tsconfig.base.json ./
COPY sdk/ts sdk/ts
COPY ui ui
COPY products/midgard/ui products/midgard/ui
ARG GATEWAY_URL=http://127.0.0.1:7710/api/v1
RUN pnpm install --frozen-lockfile --filter "@midgard/console-midgard..." \
    && VITE_GATEWAY=http VITE_GATEWAY_URL="$GATEWAY_URL" \
       pnpm --filter @midgard/console-midgard build
# The content security policy admits the page's inline scripts by their SHA-256, taken
# from the built page itself, so the policy cannot drift from the HTML it guards.
COPY deploy/installer/console.nginx.conf /src/console.nginx.conf
RUN node -e ' \
      const fs = require("fs"), crypto = require("crypto"); \
      const html = fs.readFileSync("products/midgard/ui/dist/index.html", "utf8"); \
      const hashes = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)] \
        .map((m) => "\x27sha256-" + crypto.createHash("sha256").update(m[1]).digest("base64") + "\x27"); \
      const conf = fs.readFileSync("/src/console.nginx.conf", "utf8").replaceAll("__INLINE_SCRIPTS__", hashes.join(" ")); \
      fs.writeFileSync("/src/console.nginx.conf", conf); \
      console.log("inline scripts admitted:", hashes.length);'

FROM ${RUNTIME}
COPY --from=build /src/console.nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/products/midgard/ui/dist/ /usr/share/nginx/html/
EXPOSE 8080
