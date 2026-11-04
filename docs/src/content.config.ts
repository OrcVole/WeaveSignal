// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Starlight's schema, extended with the AIo v3 frontmatter every page carries.
import { defineCollection } from 'astro:content';
import { z } from 'astro/zod';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({
    loader: docsLoader(),
    schema: docsSchema({
      extend: z.object({
        type: z.enum(['tutorial', 'howto', 'reference', 'explanation']),
        status: z.enum(['draft', 'decided', 'deprecated']),
        last_verified: z.coerce.date(),
        superseded_by: z.string().optional(),
      }),
    }),
  }),
};
