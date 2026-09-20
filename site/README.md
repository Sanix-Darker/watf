# Static site

This directory contains the public watf site. It uses plain HTML and CSS. It has
no build step, JavaScript, remote asset, analytics service, or runtime dependency.

Serve `site/` as the document root. Keep production DNS, certificates, paths, and
deployment commands in operator-managed infrastructure. The tracked Caddy example
uses `example.com` and `/srv/example-site` on purpose.

Before a release, run `python3 scripts/verify.py` and perform a real public `GET`
after the operator deploys the files. Do not use a successful `HEAD` request as
the only public check.
