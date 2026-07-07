// Ambient module declarations for build/lint tooling that ships no bundled
// TypeScript types. Included by tsconfig.node.json (the config-file scope:
// vite.config.ts + eslint.config.ts) so the maximal-strict `tsc` program can
// import them without TS7016 ("could not find a declaration file").
//
// eslint-plugin-jsx-a11y (P6 React stack) has no `@types/*` package and no
// bundled `.d.ts`. eslint.config.ts imports its default export and immediately
// casts it to the minimal `{ flatConfigs: { recommended } }` shape it uses, so
// an untyped (`any`) module here is deliberate and safe — the cast at the use
// site is the actual type contract.
declare module "eslint-plugin-jsx-a11y"
