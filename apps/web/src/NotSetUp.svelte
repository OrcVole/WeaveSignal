<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The station's own address before the station is set up. A person who installed the station
  // and opened it lands here, and the player's page would tell them nothing about where the
  // settings are. It asks the station nothing (through Cloudron's login an admin route's refusal
  // is turned into a redirect) and links with plain anchors.
  import BrandMark from './lib/BrandMark.svelte';

  const guide = 'https://weavesignal.top/docs/how-to';
  // This address, for the steps that say where to type it.
  const address = globalThis.location?.origin ?? '';
</script>

<div class="page">
  <header class="top"><BrandMark /><span class="station">Not set up yet</span></header>
  <main class="card" aria-labelledby="nsu-h">
    <h1 id="nsu-h">This station is not set up yet</h1>
    <p class="small">
      This is the station’s own address, the one you run it from. There is no player to show here until the station is set up. There are three steps.
    </p>
    <ol class="steps">
      <li>
        <b>Set the station up.</b> Open <a href="/setup/">{address}/setup/</a> and sign in with the station key. It is the line <code>CONTROL_ADMIN_TOKEN</code>.
        Where to find it:
        <ul class="where">
          <li><b>Cloudron:</b> in <code>/app/data/secrets.env</code>. Open the app’s File Manager in Cloudron and copy the value after the <code>=</code>.</li>
          <li>
            <b>Linux with Compose or Quadlets:</b> in the settings file that <code>weavesignal.sh</code> wrote: <code>deploy/compose/.env</code>, or the file you gave
            <code>--env-file</code> (the Quadlets guide uses <code>~/.config/weavesignal/station.env</code>). It is never printed. With Quadlets it is also the Podman secret
            <code>weavesignal-control-admin-token</code>; <code>podman secret inspect --showsecret</code> shows it.
          </li>
          <li>
            <b>Any other install:</b> it is the control plane’s environment variable <code>WEAVESIGNAL_CONTROL_ADMIN_TOKEN</code>, which you or your installer set. It is never in
            the settings file. If it is not set, set it to a long random string and restart the control plane.
          </li>
        </ul>
      </li>
      <li>
        <b>Install the studio app</b> on the computer you will present from:
        <a href="{guide}/install-studio-windows/" rel="noopener noreferrer">Windows</a> or
        <a href="{guide}/install-studio-linux/" rel="noopener noreferrer">Linux</a>.
      </li>
      <li>
        <b>Connect the studio app to this address,</b> <code>{address}</code>, with the same key. The address is the one in your browser’s address bar now; copy it from there or from here.
        <a href="{guide}/install-on-cloudron/#sign-in" rel="noopener noreferrer">The guide to signing in</a> shows each screen.
      </li>
    </ol>
    <a class="btn primary wide" href="/setup/">Open the setup page</a>
  </main>
</div>

<style>
  h1 {
    font-size: var(--fs-24);
  }
  .steps {
    margin: 0;
    padding-left: var(--s-5);
    display: grid;
    gap: var(--s-3);
    line-height: var(--lh-body);
  }
  .where {
    margin: var(--s-2) 0 0;
    padding-left: var(--s-5);
    display: grid;
    gap: var(--s-2);
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.9em;
    background: var(--surface-2);
    padding: 0.1em 0.3em;
    border-radius: var(--r-1);
    overflow-wrap: anywhere;
  }
  a.btn {
    text-decoration: none;
  }
</style>
