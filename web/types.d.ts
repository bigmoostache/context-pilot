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

// eslint-plugin-promise (P7 correctness stack) likewise ships no bundled types
// and has no `@types/*` package. eslint.config.ts imports its default export
// and casts it to the minimal `{ configs: { "flat/recommended" } }` shape it
// uses at the use site — so an untyped (`any`) module here is deliberate and
// safe, the cast being the real type contract.
declare module "eslint-plugin-promise"

// eslint-plugin-security (P7 correctness stack) likewise ships no bundled types
// and has no `@types/*` package. eslint.config.ts imports its default export
// only to read `.configs.recommended` (a flat config object) — an untyped
// (`any`) module here is deliberate; the plugin's own recommended preset is the
// contract, and every rule level it sets is overridden explicitly in the config.
declare module "eslint-plugin-security"

// eslint-plugin-no-unsanitized (P7 correctness stack) likewise ships no bundled
// types and has no `@types/*` package. eslint.config.ts imports its default
// export only to read `.configs.recommended` (a flat `{ plugins, rules }`
// object) — an untyped (`any`) module here is deliberate; the cast at the use
// site to the minimal `{ configs: { recommended } }` shape is the real type
// contract, and its two rules are enabled by that recommended preset.
declare module "eslint-plugin-no-unsanitized"

// eslint-plugin-boundaries (P9 architecture layers) ships no bundled types and
// has no `@types/*` package. eslint.config.ts imports its default export only
// to register it as a plugin (cast to ESLint's `Plugin` shape at the use site);
// its behaviour is driven entirely by `settings["boundaries/elements"]` + the
// `boundaries/dependencies` rule, not by the plugin object's own type — so an
// untyped (`any`) module here is deliberate and safe.
declare module "eslint-plugin-boundaries"
