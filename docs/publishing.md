# Publishing

Benchmark runs are produced locally. Cloudflare only builds and deploys the
static Worker site from `master`.

## Cloudflare Workers Build

Connect the repository with the Cloudflare Workers & Pages GitHub App, then use
these build settings:

- Root directory: `.`
- Build command: `npm --prefix report-ui ci && npm --prefix report-ui run build`
- Deploy command: `npx wrangler deploy`
- Worker name: `evm-compiler-bench`

The Worker serves `report-ui/dist` as static assets and reads benchmark result
objects from the `evm-compilers` R2 bucket.

## Local Result Publish

After a local benchmark run:

```sh
cargo run --release -- run
cargo run --release -- validate
just publish-results
```

`just publish-results` compresses the current files under `results/`, uploads
immutable run artifacts under `evm-compiler-bench/runs/{run_id}/`, and updates
`evm-compiler-bench/latest.json` in R2.

The publish command uses remote R2, so Wrangler must be authenticated locally:

```sh
wrangler login
```

Alternatively, set `CLOUDFLARE_API_TOKEN` to a token with permission to write
objects to the `evm-compilers` bucket.

Do not commit generated result blobs or built static assets.
