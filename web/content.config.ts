import { defineCollection, defineContentConfig, z } from "@nuxt/content";

export default defineContentConfig({
  collections: {
    docs: defineCollection({
      type: "page",
      source: "docs/**/*.md",
      schema: z.object({
        section: z.enum(["Get started", "Guides", "Concepts", "Reference"]),
        order: z.number(),
        banner: z.string().optional(),
      }),
    }),
  },
});
