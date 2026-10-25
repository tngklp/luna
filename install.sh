#!/usr/bin/env sh
#
# Interactive installer for the Luna terminal interface.
#
# Downloads a prebuilt `luna` binary from GitHub Releases, verifies its checksum,
# installs it under ~/.local/bin, adds it to PATH, and walks through provider
# setup. No Rust toolchain is required.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/tngklp/luna/main/install.sh | sh
#   ./install.sh --version v0.1.0 --yes
#
set -eu

REPO="tngklp/luna"
BINARY="luna"
VERSION=""
INSTALL_DIR=""
ASSUME_YES=0
SKIP_PATH=0

# --------------------------------------------------------------------- output

if [ -t 1 ] && [ "${NO_COLOR:-}" = "" ]; then
  BOLD=$(printf '\033[1m')
  DIM=$(printf '\033[2m')
  CYAN=$(printf '\033[36m')
  GREEN=$(printf '\033[32m')
  YELLOW=$(printf '\033[33m')
  RED=$(printf '\033[31m')
  MAGENTA=$(printf '\033[35m')
  RESET=$(printf '\033[0m')
else
  BOLD=""; DIM=""; CYAN=""; GREEN=""; YELLOW=""; RED=""; MAGENTA=""; RESET=""
fi

step() { printf '\n  %s%s%s\n' "$CYAN" "$1" "$RESET"; }
ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$1"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$1"; }
fail() { printf '  %s✗%s %s\n' "$RED" "$RESET" "$1" >&2; }
die()  { fail "$1"; exit 1; }

banner() {
  printf '\n'
  printf '   %s██╗   ██╗███╗   ██╗███╗   ██╗ █████╗ %s\n' "$MAGENTA" "$RESET"
  printf '   %s██║   ██║████╗  ██║████╗  ██║██╔══██╗%s\n' "$MAGENTA" "$RESET"
  printf '   %s██║   ██║██╔██╗ ██║██╔██╗ ██║███████║%s\n' "$MAGENTA" "$RESET"
  printf '   %s██║   ██║██║╚██╗██║██║╚██╗██║██╔══██║%s\n' "$MAGENTA" "$RESET"
  printf '   %s╚██████╔╝██║ ╚████║██║ ╚████║██║  ██║%s\n' "$MAGENTA" "$RESET"
  printf '   %s ╚═════╝ ╚═╝  ╚═══╝╚═╝  ╚═══╝╚═╝  ╚═╝%s\n' "$MAGENTA" "$RESET"
  printf '\n   %sAI coding agent — terminal installer%s\n\n' "$DIM" "$RESET"
}

# ---------------------------------------------------------------------- args

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:-}"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir) INSTALL_DIR="${2:-}"; shift 2 ;;
    --dir=*) INSTALL_DIR="${1#*=}"; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --no-path) SKIP_PATH=1; shift ;;
    --help|-h)
      cat <<'EOF'
Luna installer

  --version <tag>   Install a specific release, e.g. v0.1.0
  --dir <path>      Install directory (default: ~/.local/bin)
  --yes, -y         Accept defaults, skip prompts
  --no-path         Do not modify PATH
  --help, -h        Show this help
EOF
      exit 0
      ;;
    *) die "Unknown option: $1" ;;
  esac
done

# ------------------------------------------------------------------ prompting

# read_choice <prompt> <default-index> <option>...
#
# Everything except the chosen index goes to stderr, because the caller captures
# stdout: `choice=$(read_choice ...)`.
read_choice() {
  prompt="$1"; default="$2"; shift 2
  printf '\n  %s\n' "$prompt" >&2
  index=0
  for option in "$@"; do
    if [ "$index" -eq "$default" ]; then marker='*'; else marker=' '; fi
    printf '   %s %d. %s\n' "$marker" "$((index + 1))" "$option" >&2
    index=$((index + 1))
  done
  if [ "$ASSUME_YES" -eq 1 ]; then
    printf '  %s→ default%s\n' "$DIM" "$RESET" >&2
    printf '%s' "$default"
    return
  fi
  while :; do
    printf '  Choice [%d]: ' "$((default + 1))" >&2
    read -r answer || answer=""
    if [ -z "$answer" ]; then printf '%s' "$default"; return; fi
    case "$answer" in
      ''|*[!0-9]*) warn "Enter a number." ;;
      *)
        if [ "$answer" -ge 1 ] && [ "$answer" -le "$#" ]; then
          printf '%s' "$((answer - 1))"
          return
        fi
        warn "Enter a number between 1 and $#."
        ;;
    esac
  done
}

# read_text <prompt> <default> — prompt on stderr, answer on stdout.
read_text() {
  if [ "$ASSUME_YES" -eq 1 ]; then printf '%s' "$2"; return; fi
  if [ -z "$2" ]; then
    printf '  %s: ' "$1" >&2
  else
    printf '  %s [%s]: ' "$1" "$2" >&2
  fi
  read -r answer || answer=""
  if [ -z "$answer" ]; then printf '%s' "$2"; else printf '%s' "$answer"; fi
}

# read_secret <prompt> — never echoes
read_secret() {
  if [ "$ASSUME_YES" -eq 1 ]; then printf ''; return; fi
  printf '  %s: ' "$1" >&2
  stty -echo 2>/dev/null || true
  read -r answer || answer=""
  stty echo 2>/dev/null || true
  printf '\n' >&2
  printf '%s' "$answer"
}

# ------------------------------------------------------------------- platform

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Linux)  os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) die "Unsupported operating system: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *) die "Unsupported architecture: $arch" ;;
  esac

  printf '%s-%s' "$arch_part" "$os_part"
}

resolve_version() {
  if [ -n "$VERSION" ]; then
    case "$VERSION" in v*) printf '%s' "$VERSION" ;; *) printf 'v%s' "$VERSION" ;; esac
    return
  fi
  printf '  %sLooking up the latest release…%s\n' "$DIM" "$RESET" >&2
  tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$tag" ] || die "Could not determine the latest release. Pass --version."
  printf '%s' "$tag"
}

download() {
  url="$1"; dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$dest"
  else
    die "Neither curl nor wget is available."
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    printf ''
  fi
}

# ------------------------------------------------------------------- install

install_luna() {
  triple=$(detect_target)
  tag=$(resolve_version)
  ok "Target: $triple"
  ok "Version: $tag"

  archive="luna-$triple.tar.gz"
  base="https://github.com/$REPO/releases/download/$tag"
  temp=$(mktemp -d)
  trap 'rm -rf "$temp"' EXIT INT TERM

  step "Downloading $archive"
  if ! download "$base/$archive" "$temp/$archive" 2>/dev/null; then
    case "$triple" in
      aarch64-unknown-linux-gnu)
        die "No ARM64 Linux build is published for $tag. Build from source: cargo build --release -p luna-tui"
        ;;
      *)
        die "Could not download $archive from $base — the release may not include this platform yet."
        ;;
    esac
  fi
  size=$(wc -c < "$temp/$archive" | tr -d ' ')
  ok "Downloaded $((size / 1024 / 1024)) MB"

  step "Verifying the download"
  if download "$base/checksums.txt" "$temp/checksums.txt" 2>/dev/null; then
    expected=$(awk -v name="$archive" '$2 == name { print $1 }' "$temp/checksums.txt" | head -n 1)
    actual=$(sha256_of "$temp/$archive")
    if [ -n "$expected" ] && [ -n "$actual" ]; then
      if [ "$expected" != "$actual" ]; then
        die "Checksum mismatch: expected $expected, got $actual"
      fi
      ok "Checksum verified"
    else
      warn "No checksum published for this archive"
    fi
  else
    warn "checksums.txt is not available for this release"
  fi

  step "Extracting"
  tar -xzf "$temp/$archive" -C "$temp" || die "Could not extract the archive"
  binary=$(find "$temp" -type f -name "$BINARY" | head -n 1)
  [ -n "$binary" ] || die "$BINARY was not found in the archive"

  if [ -z "$INSTALL_DIR" ]; then
    INSTALL_DIR="$HOME/.local/bin"
  fi
  mkdir -p "$INSTALL_DIR"
  install -m 755 "$binary" "$INSTALL_DIR/$BINARY" 2>/dev/null \
    || { cp "$binary" "$INSTALL_DIR/$BINARY" && chmod 755 "$INSTALL_DIR/$BINARY"; }
  ok "Installed to $INSTALL_DIR/$BINARY"

  if [ "$SKIP_PATH" -eq 0 ]; then
    add_to_path "$INSTALL_DIR"
  fi

  step "Verifying the install"
  if reported=$("$INSTALL_DIR/$BINARY" --version 2>&1); then
    ok "luna $reported"
  else
    warn "The binary did not report a version: $reported"
  fi
}

add_to_path() {
  dir="$1"
  case ":$PATH:" in
    *":$dir:"*)
      ok "Already on PATH"
      return
      ;;
  esac

  # Pick the rc file the user's shell actually reads.
  shell_name=$(basename "${SHELL:-sh}")
  case "$shell_name" in
    zsh)  rc="$HOME/.zshrc" ;;
    bash) rc="$HOME/.bashrc" ;;
    fish) rc="$HOME/.config/fish/config.fish" ;;
    *)    rc="$HOME/.profile" ;;
  esac

  line="export PATH=\"$dir:\$PATH\""
  if [ "$shell_name" = "fish" ]; then
    line="set -gx PATH $dir \$PATH"
  fi

  if [ -f "$rc" ] && grep -F "$dir" "$rc" >/dev/null 2>&1; then
    ok "Already in $rc"
    return
  fi

  mkdir -p "$(dirname "$rc")"
  printf '\n# Added by the Luna installer\n%s\n' "$line" >> "$rc"
  ok "Added to PATH in $rc (open a new shell to use it)"
}

# ------------------------------------------------------------------ provider

configure_provider() {
  step "Provider setup"
  printf '  %sLuna needs one model provider. A local model needs no key.%s\n' "$DIM" "$RESET"

  choice=$(read_choice "Which provider will you use?" 0 \
    "Anthropic (Claude)" \
    "OpenAI (GPT)" \
    "Google Gemini" \
    "Local (Ollama or llama.cpp)" \
    "Skip for now")

  # Must match what `directories::ProjectDirs` resolves to, which is where the
  # app looks: ~/.config/luna/config.toml on Linux, ~/Library/Application
  # Support/luna/config.toml on macOS.
  case "$(uname -s)" in
    Darwin) config_dir="$HOME/Library/Application Support/luna" ;;
    *)      config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/luna" ;;
  esac
  config_path="$config_dir/config.toml"
  mkdir -p "$config_dir"

  case "$choice" in
    0) provider="anthropic" ;;
    1) provider="openai" ;;
    2) provider="gemini" ;;
    3) provider="local" ;;
    *) warn "Skipped. Set an API key later, or edit $config_path"; return ;;
  esac

  case "$provider" in
    anthropic) env_var="ANTHROPIC_API_KEY" ;;
    openai)    env_var="OPENAI_API_KEY" ;;
    gemini)    env_var="GEMINI_API_KEY" ;;
    *)         env_var="" ;;
  esac

  if [ -n "$env_var" ]; then
    current=$(eval "printf '%s' \"\${$env_var:-}\"")
    if [ -n "$current" ]; then
      ok "$env_var is already set"
    else
      key=$(read_secret "Paste your $env_var (blank to skip)")
      if [ -n "$key" ]; then
        # Persist to the same rc file the PATH entry went to.
        shell_name=$(basename "${SHELL:-sh}")
        case "$shell_name" in
          zsh)  rc="$HOME/.zshrc" ;;
          bash) rc="$HOME/.bashrc" ;;
          fish) rc="$HOME/.config/fish/config.fish" ;;
          *)    rc="$HOME/.profile" ;;
        esac
        if [ "$shell_name" = "fish" ]; then
          printf '\nset -gx %s "%s"\n' "$env_var" "$key" >> "$rc"
        else
          printf '\nexport %s="%s"\n' "$env_var" "$key" >> "$rc"
        fi
        chmod 600 "$rc" 2>/dev/null || true
        ok "$env_var saved to $rc"
      else
        warn "No key saved. Set $env_var before running luna."
      fi
    fi
  else
    url=$(read_text "Local server URL" "http://localhost:11434/v1")
    ok "Using $url"
  fi

  if [ ! -f "$config_path" ]; then
    case "$provider" in
      anthropic) default_model="anthropic/claude-sonnet-4-5" ;;
      openai)    default_model="openai/gpt-5" ;;
      gemini)    default_model="gemini/gemini-2.5-pro" ;;
      *)         default_model="local/llama3.2" ;;
    esac
    cat > "$config_path" <<EOF
[general]
default_model = "$default_model"
max_concurrent_agents = 4
max_steps = 60

[approvals]
read = "allow"
write = "ask"
bash = "ask"
network = "allow"
EOF
    ok "Wrote $config_path"
  else
    ok "Kept the existing $config_path"
  fi
}

# ---------------------------------------------------------------------- main

banner

step "Checking your system"
ok "$(uname -s) $(uname -r)"
ok "$(uname -m)"

install_luna
configure_provider

printf '\n  %s─────────────────────────────────────────────%s\n' "$DIM" "$RESET"
ok "Luna is installed"
printf '\n  Start it with:\n    %sluna%s\n\n' "$CYAN" "$RESET"
printf '  %sIf the command is not found, open a new shell.%s\n' "$DIM" "$RESET"
printf '  %sWant the desktop app too? Install the Luna setup from:%s\n' "$DIM" "$RESET"
printf '    %shttps://github.com/%s/releases/latest%s\n\n' "$DIM" "$REPO" "$RESET"
