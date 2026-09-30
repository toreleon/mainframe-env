# Public project page

The public project page is <https://toreleon.github.io/mainframe-env/>.
It is a static site with no build dependencies, remote fonts, analytics, or
runtime services. `index.html`, `styles.css`, and `favicon.svg` are the page;
`.nojekyll` keeps GitHub Pages from processing it with Jekyll.

Only this directory is published. The repository remains private, and the page
labels repository, documentation, and release links as requiring access.
Published status copy must remain consistent with the root README and release
records; public page publication grants no production or licensed conformance
credit.

## Preview

From the repository root, run the project Python interpreter:

```bash
"$(tools/jenkins/select-python.sh)" -B -m http.server 8080 --directory site
```

Open <http://localhost:8080/>. Review both desktop and narrow mobile layouts.

## Publish an update

GitHub Pages publishes the root of the `gh-pages` branch. Keep source edits in
`site/` on a normal feature branch, run relevant documentation and policy gates,
and commit the source before publishing. From that committed checkout:

```bash
git fetch origin gh-pages
page_commit=$(git commit-tree HEAD:site -p origin/gh-pages \
  -m "docs: update public project page")
git push origin "$page_commit:refs/heads/gh-pages"
```

This creates a normal descendant of the previous page commit containing only
the `site/` tree; no force push or source history is needed. If another publisher
updates the branch first, the push fails: fetch again, review the competing
change, and recreate the deployment commit. Do not publish repository roots,
IBM publication bodies, caches, build targets, or execution receipts.

Confirm the Pages build succeeded and the public page serves the expected
HTML and assets before reporting publication complete. Future commits pushed
to `gh-pages` publish automatically through GitHub's branch publishing service;
there is no additional tracked Actions workflow.
