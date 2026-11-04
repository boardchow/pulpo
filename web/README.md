# pulpo.sh

The landing page and the docs, in Nuxt, generated as static HTML.

```sh
npm install
npm run dev        # http://localhost:3000, reloading as you edit
npm run generate   # the site, in .output/public, for any static host
```

The docs are Markdown in `content/docs`, one file per page: `title`,
`description`, `section` and `order` at the top place it in the list.
In the text, `:k[esc]` draws a key and `:kp[n]` the prefix then a key
(`:kp{k="["}` for a bracket).

`public/install` is the installer behind `curl -fsSL https://pulpo.sh/install | sh`.
