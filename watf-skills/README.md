# watf skill

Agent instructions for using [watf](https://github.com/Sanix-Darker/watf) as a
local CLI capability, validation, and bounded execution interface.

Install the skill:

```sh
npx skills add sanix-darker/watf-skills --skill watf
```

The skill requires exactly `watf 0.0.1`. If `watf` is absent or reports another
version, the bootstrap reinstalls the pinned crate with `--force`. It creates the
default local index only when that index is absent:

```sh
cargo install watf --version 0.0.1 --locked --force
[ -f "${WATF_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/watf}/index.widx" ] || watf index
```

The default crate has no model dependency. Local model planning is optional and
requires a compatible feature build plus a separately provisioned GGUF model.
The crate embeds core records only; the offline cloud catalog is separate data.
