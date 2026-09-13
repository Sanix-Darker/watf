# Corpus provenance and regeneration

The bulk catalog is derived from the locally available botocore 1.43.18 service
models. Upstream: https://github.com/boto/botocore . The source package's Apache-2.0
license is copied to `third_party/botocore-LICENSE.txt`.

`scripts/build_catalog.py` selects the latest bundled API model per service,
uses botocore's CLI-style name conversion, maps each operation to a command,
and maps direct input members to options. S3 service operations use `aws s3api`,
not the customized high-level `aws s3` namespace. Shared nested structures are
visited once per service; each structure member becomes a typed input_field.
Aliases and enums remain metadata rather than additional counted records.

The model corpus contains 18,467 commands, 64,450 options and 41,613 input fields:
124,530 unique records over 425 service models. It is not 100,000 installed
programs, independent tasks, verified command lines or documentation pages.
The cloud-heavy corpus exists alongside 608 conservative bootstrap records covering
117 common command scopes. The latter are a subset of their tools' interfaces.

## Limitations of service models

A botocore service model is not the complete AWS CLI implementation. Custom
commands, global options, custom parameter handling, plugins and installed-version
differences exist. Root executable presence does not prove a model-derived operation
is available. Nested JSON field documentation cannot be used as a shell flag.
Use current captured CLI documentation when installed syntax matters.

The manifest identifies the package version, API versions and every source model's
compressed and decoded SHA256. It also hashes the generated compressed and decoded
catalog. These hashes support reproducibility/corruption checks, not authenticity
against a compromised upstream package or malicious same-origin release.

## Regenerate explicitly

Python and botocore are maintainer-time dependencies only:

```sh
python3 -m venv .venv
. .venv/bin/activate
python3 -m pip install botocore==1.43.18
python3 scripts/build_catalog.py
python3 scripts/build_core.py
python3 scripts/build_samples.py
python3 scripts/build_schemas.py
python3 scripts/verify.py
```

Review the manifest and data diff when changing upstream versions. Do not update
source versions by simply editing a manifest. Do not add synthetically permuted
flags, aliases, enum values or duplicated command strings to reach a target count.
Keep nested fields and executable option surfaces separately counted.

Local documentation may have a different license and may contain private names
or proprietary material. Do not publish an index of private local docs without
reviewing permissions. Source paths and snippets appear in evidence packets.
