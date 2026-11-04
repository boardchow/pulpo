import { defineCollection, defineContentConfig, z } from "@nuxt/content";

// Each page of the docs: its place in the list (a section, then an order).
export default defineContentConfig({
  collections: {
    docs: defineCollection({
      type: "page",
      source: "docs/**/*.md",
      schema: z.object({
        section: z.enum(["Get started", "Using Pulpo", "Reference"]),
        order: z.number(),
      }),
    }),
  },
});
