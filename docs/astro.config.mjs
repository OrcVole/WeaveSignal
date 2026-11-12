// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// WeaveSignal's website: a hand-built landing site at the root (src/pages) and the Starlight
// documentation (ADR-0072: the four kinds, panel names as headings) under /docs/. The documentation
// lived at the root until 2026-11-04; every old address redirects to its new one (static pages
// with a meta refresh and a canonical link, since the host serves files and nothing else).
import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// Visit counting (operator, 2026-11-04): a cookieless Umami script, only when the deploy sets
// PUBLIC_UMAMI_SRC and PUBLIC_UMAMI_WEBSITE_ID, so the source names no analytics host.
const umami = process.env.PUBLIC_UMAMI_SRC && process.env.PUBLIC_UMAMI_WEBSITE_ID
  ? [{ tag: 'script', attrs: { defer: true, src: process.env.PUBLIC_UMAMI_SRC, 'data-website-id': process.env.PUBLIC_UMAMI_WEBSITE_ID } }]
  : [];

// Every documentation page's old address (/<kind>/<slug>/) mapped to its new one (/docs/<kind>/<slug>/),
// read from the content folder when the configuration loads, so a page added later is covered.
function docsRedirects() {
  const root = new URL('./src/content/docs/docs', import.meta.url).pathname;
  const map = {};
  const walk = (dir, rel) => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      if (statSync(full).isDirectory()) {
        walk(full, `${rel}${name}/`);
      } else if (/\.mdx?$/.test(name) && name !== 'index.md' && name !== 'index.mdx') {
        const slug = `${rel}${name.replace(/\.mdx?$/, '')}/`;
        map[`/${slug}`] = `/docs/${slug}`;
      }
    }
  };
  for (const kind of ['tutorials', 'how-to', 'reference', 'explanation']) walk(join(root, kind), `${kind}/`);
  return map;
}

export default defineConfig({
  site: 'https://weavesignal.top',
  trailingSlash: 'always',
  redirects: {
    ...docsRedirects(),
    // the page renamed on 2026-11-04 ("desk B" is an internal design name)
    '/how-to/present-at-desk-b/': '/docs/how-to/present-from-the-keyboard/',
    '/docs/how-to/present-at-desk-b/': '/docs/how-to/present-from-the-keyboard/',
    // the studio app's downloads are on the "run your own station" page
    '/download/': '/run/#studio-app',
  },
  integrations: [
    starlight({
      title: 'WeaveSignal docs',
      description: 'The documentation of WeaveSignal, open-source internet radio software with the live studio built in: tutorials, how-to guides, reference and explanation.',
      // The app icon (ADR-0094) beside the title; Boxicons' broadcast glyph on the dark surface.
      logo: { src: './src/assets/weavesignal-app-icon.svg', alt: '' },
      social: [
        { icon: 'forgejo', label: 'Source code on Forgejo', href: 'https://forgejo.weavesignal.top/WeaveSignal/weavesignal' },
        { icon: 'github', label: 'Mirror on GitHub', href: 'https://github.com/OrcVole/WeaveSignal' },
      ],
      head: [
        { tag: 'meta', attrs: { property: 'og:image', content: 'https://weavesignal.top/og.png' } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:alt', content: "The studio app's desk: ON AIR for The Morning Kettle on Kettle FM (sample data), three people on air, two standing by." } },
        ...umami,
      ],
      // The licence line under every page (ADR-0081).
      components: {
        Footer: './src/components/Footer.astro',
      },
      // The dark and light grounds the state chips are shown on (reference/states), then the
      // product's colours and faces for the documentation.
      customCss: ['./src/styles/states.css', './src/styles/site.css'],
      sidebar: [
        // The simple guides for people (docs/WRITING-GUIDES.md): the short road through the full pages.
        { label: 'Simple guides', items: [{ autogenerate: { directory: 'guides' } }] },
        { label: 'Overview', link: '/docs/' },
        // The install wizard (ADR-0237): a page that runs in the browser, src/pages/docs/install-wizard.astro.
        { label: 'Install wizard', link: '/docs/install-wizard/' },
        { label: 'Tutorials', items: [{ autogenerate: { directory: 'docs/tutorials' } }] },
        { label: 'How-to guides', items: [{ autogenerate: { directory: 'docs/how-to' } }] },
        { label: 'Reference', items: [{ autogenerate: { directory: 'docs/reference' } }] },
        { label: 'Explanation', items: [{ autogenerate: { directory: 'docs/explanation' } }] },
        {
          label: 'The website',
          items: [
            { label: 'Home', link: '/' },
            { label: 'Features', link: '/features/' },
            { label: 'Compare', link: '/compare/' },
            { label: 'Questions', link: '/faq/' },
            { label: 'Run your own station', link: '/run/' },
          ],
        },
      ],
    }),
  ],
});
