// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// WeaveSignal's documentation site (ADR-0072): the four kinds, panel names as headings.
// It is also the product's website, served at the root of weavesignal.top.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://weavesignal.top',
  integrations: [
    starlight({
      title: 'WeaveSignal',
      description: 'A radio and video station studio: the live room, the desk, playout and listener streams.',
      // The app icon (ADR-0094) beside the title; Boxicons' broadcast glyph on the dark surface.
      logo: { src: './src/assets/weavesignal-app-icon.svg', alt: '' },
      social: [
        { icon: 'forgejo', label: 'Source code on Forgejo', href: 'https://forgejo.weavesignal.top/WeaveSignal/weavesignal' },
        { icon: 'github', label: 'Source code on GitHub', href: 'https://github.com/OrcVole/WeaveSignal' },
      ],
      // The licence line under every page (ADR-0081), and the landing page's hero.
      components: {
        Footer: './src/components/Footer.astro',
        Hero: './src/components/Hero.astro',
      },
      // The dark and light grounds the state chips are shown on (reference/states), then the
      // product's colours for the site and the landing page's layout.
      customCss: ['./src/styles/states.css', './src/styles/site.css'],
      sidebar: [
        { label: 'Tutorials', items: [{ autogenerate: { directory: 'tutorials' } }] },
        { label: 'How-to guides', items: [{ autogenerate: { directory: 'how-to' } }] },
        { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
        { label: 'Explanation', items: [{ autogenerate: { directory: 'explanation' } }] },
      ],
    }),
  ],
});
