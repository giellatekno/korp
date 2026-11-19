// Instance settings (see korp-vue's docs/INSTANCE.md). The settings of one
// language: the build copies gtweb2_config/front/config-<lang>.yaml to
// config.yml here (see manage/containerfiles/Containerfile.frontend).
import type { InstanceConfig } from "@/core/config/instanceConfig.types"
import config from "@instance/config.yml"

const settings = config as InstanceConfig

// The API is served by the same Worker as the frontend, at api/ next to the
// page (see manage/worker/src/index.ts). Computed here, in the browser, so the
// same build works wherever it is served: https://korp-<lang>.….workers.dev/
// or behind a reverse proxy at some /path/. (korp-vue requires an absolute URL.)
settings.korp_backend_url = new URL("api", new URL(".", window.location.href)).href

export default settings
