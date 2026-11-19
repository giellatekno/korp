// Instance plugin (see korp-vue's docs/INSTANCE.md): Giellatekno's branding.
import type { Plugin } from "vue"
import { componentInjectionKeys } from "@/injection"
import BrandSecondary from "@instance/components/BrandSecondary.vue"
import FooterContent from "@instance/components/FooterContent.vue"

export default async function createPlugin(): Promise<Plugin> {
  return (app) => {
    // Header, right of the Korp logo: UiT and Giellatekno
    app.provide(componentInjectionKeys.BrandSecondary, BrandSecondary)
    // Instead of Språkbanken's footer
    app.provide(componentInjectionKeys.FooterContent, FooterContent)
  }
}
