Vendored browser bundles used by the static HTML report.

- `vega-6.2.0.min.js`: https://cdn.jsdelivr.net/npm/vega@6.2.0/build/vega.min.js
- `vega-lite-6.4.3.min.js`: https://cdn.jsdelivr.net/npm/vega-lite@6.4.3/build/vega-lite.min.js
- `vega-embed-7.1.0.min.js`: https://cdn.jsdelivr.net/npm/vega-embed@7.1.0/build/vega-embed.min.js

All three packages are BSD-3-Clause licensed; the vendored license text is in
`VEGA-LICENSE-BSD-3-Clause.txt`.

The report generator copies these files into `results/reports/assets/` so the
generated report can be opened from `file://` without CDN access.
