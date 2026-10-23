# Redirect-aware installer endpoint.
#
# These URLs are served by GitHub Pages / your host of choice:
#   https://luna.tngklp.com/install      -> raw install.sh (POSIX)
#   https://luna.tngklp.com/install.ps1  -> raw install.ps1 (Windows)
#
# The one-liners:
#   POSIX:   curl -fsSL https://luna.tngklp.com/install | sh
#   Windows: irm https://luna.tngklp.com/install.ps1 | iex
#
# Setup: point these paths at the raw GitHub files, e.g. with a Pages site
# containing two redirect files, or CNAME-level URL rewrites to:
#   https://raw.githubusercontent.com/tngklp/luna/main/install.sh
#   https://raw.githubusercontent.com/tngklp/luna/main/install.ps1
