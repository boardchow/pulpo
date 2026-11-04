import { defineCollection, defineContentConfig, z } from "@nuxt/content";

export default defineContentConfig({
  collections: {
    docs: defineCollection({
      type: "page",
      source: "docs/**/*.md",
      schema: z.object({
        section: z.enum(["Get started", "Using Pulpo", "Task sessions", "Reference"]),
        order: z.number(),
      }),
    }),
  },
});
