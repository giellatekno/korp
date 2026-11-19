<script setup lang="ts">
import uitLogo from "@instance/assets/UiT_Segl_Eng_Sort_960px.png"
import gtLogo from "@instance/assets/giellatekno_logo_official.svg"
</script>

<template>
  <div class="d-none d-lg-flex align-items-center gap-3 pe-2">
    <a href="https://uit.no/" target="_blank">
      <img :src="uitLogo" alt="UiT The Arctic University of Norway" style="height: 72px" />
    </a>
    <a href="https://giellatekno.uit.no/" target="_blank">
      <img :src="gtLogo" alt="Giellatekno" style="height: 44px" />
    </a>
  </div>
</template>
