name := 'cosmic-ext-calendar'
appid := 'dev.edfloreshz.Calendar'

rootdir := ''
prefix := '/usr'
sysconfdir := '/etc'

base-dir := absolute_path(clean(rootdir / prefix))

bin-src := 'target' / 'release' / name
bin-dst := base-dir / 'bin' / name

desktop := appid + '.desktop'
desktop-dst := clean(rootdir / prefix) / 'share' / 'applications' / desktop

autostart-dst := clean(rootdir / sysconfdir) / 'xdg' / 'autostart' / (appid + '.Notifications.desktop')

appdata := appid + '.metainfo.xml'
appdata-dst := clean(rootdir / prefix) / 'share' / 'appdata' / appdata

icons-src := 'resources' / 'icons' / 'hicolor'
icons-dst := clean(rootdir / prefix) / 'share' / 'icons' / 'hicolor'

icon-svg-src := icons-src / 'scalable' / 'apps' / 'icon.svg'
icon-svg-dst := icons-dst / 'scalable' / 'apps' / appid + '.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release {{args}}

# Installs files
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 resources/app.desktop {{desktop-dst}}
    install -Dm0644 resources/notifications.desktop {{autostart-dst}}
    install -Dm0644 resources/app.metainfo.xml {{appdata-dst}}
    install -Dm0644 {{icon-svg-src}} {{icon-svg-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{icon-svg-dst}} {{autostart-dst}}

# Vendor dependencies locally
vendor:
    #!/usr/bin/env bash
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    echo >> .cargo/config.toml
    echo '[env]' >> .cargo/config.toml
    if [ -n "${SOURCE_DATE_EPOCH}" ]
    then
        source_date="$(date -d "@${SOURCE_DATE_EPOCH}" "+%Y-%m-%d")"
        echo "VERGEN_GIT_COMMIT_DATE = \"${source_date}\"" >> .cargo/config.toml
    fi
    if [ -n "${SOURCE_GIT_HASH}" ]
    then
        echo "VERGEN_GIT_SHA = \"${SOURCE_GIT_HASH}\"" >> .cargo/config.toml
    fi
    tar pcf vendor.tar .cargo vendor
    rm -rf .cargo vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Regenerates the Flatpak dependency manifest from Cargo.lock
flatpak-cargo-sources:
    #!/usr/bin/env bash
    set -euo pipefail
    scratch=$(mktemp -d flatpak/.cargo-sources.XXXXXX)
    trap 'rm -rf "$scratch"' EXIT
    curl -fsSL https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py -o "$scratch/generator.py"
    python3 -m venv "$scratch/.venv"
    "$scratch/.venv/bin/python" -m pip install 'aiohttp>=3.9.5,<4' 'tomlkit>=0.13.3,<1'
    "$scratch/.venv/bin/python" "$scratch/generator.py" Cargo.lock -o "$scratch/cargo-sources.json"
    mv "$scratch/cargo-sources.json" flatpak/cargo-sources.json

# Builds the Flatpak with the dependencies declared in its manifest
flatpak *args:
    flatpak-builder --user --force-clean {{args}} .flatpak-builder/build-dir flatpak/{{appid}}.json

# Builds an installable Flatpak bundle under dist.
flatpak-bundle *args:
    mkdir -p dist
    just flatpak --repo=.flatpak-builder/repo {{args}}
    flatpak build-bundle .flatpak-builder/repo dist/{{appid}}.flatpak {{appid}}

# Build the AppImage, zsync metadata, and checksums under dist.
appimage:
    #!/usr/bin/env bash
    set -euo pipefail
    export ARCH=x86_64
    APPIMAGE=Calendar-x86_64.AppImage
    repository="${GITHUB_REPOSITORY:-cosmic-utils/calendar}"

    if [[ "$(uname -sm)" != "Linux x86_64" ]]; then
        echo "AppImage packaging requires Linux x86_64." >&2
        exit 1
    fi
    for dependency in curl desktop-file-validate zsyncmake; do
        if ! command -v "$dependency" >/dev/null; then
            echo "Missing packaging dependency: $dependency" >&2
            exit 1
        fi
    done

    output="$PWD/dist"
    mkdir -p "$output"
    tools=$(mktemp -d "$output/.build.XXXXXX")
    trap 'rm -rf -- "$tools"' EXIT
    curl --fail --location --retry 3 \
      https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20250213-2/linuxdeploy-x86_64.AppImage \
      --output "$tools/linuxdeploy"
    curl --fail --location --retry 3 \
      https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
      --output "$tools/appimagetool"
    echo "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  $tools/appimagetool" | sha256sum --check
    chmod +x "$tools/linuxdeploy" "$tools/appimagetool"

    cargo build --locked --release --target-dir target
    install -Dm644 resources/app.metainfo.xml "$tools/AppDir/usr/share/metainfo/{{appid}}.metainfo.xml"
    install -Dm644 "{{icon-svg-src}}" "$tools/{{appid}}.svg"
    install -Dm644 resources/app.desktop "$tools/{{desktop}}"
    desktop-file-validate "$tools/{{desktop}}"
    APPIMAGE_EXTRACT_AND_RUN=1 "$tools/linuxdeploy" --appdir "$tools/AppDir" \
      --executable "target/release/{{name}}" \
      --desktop-file "$tools/{{desktop}}" \
      --icon-file "$tools/{{appid}}.svg"

    cd "$tools"
    update_info="gh-releases-zsync|${repository%/*}|${repository#*/}|latest|${APPIMAGE}.zsync"
    APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool" --updateinformation "$update_info" \
      --file-url "$APPIMAGE" AppDir "$APPIMAGE"
    test -s "$APPIMAGE.zsync"
    LC_ALL=C grep -aFxm1 "URL: $APPIMAGE" "$APPIMAGE.zsync"
    test "$(./"$APPIMAGE" --appimage-updateinformation)" = "$update_info"
    sha256sum "$APPIMAGE" "$APPIMAGE.zsync" > SHA256SUMS
    sha256sum --check SHA256SUMS
    mv -- "$APPIMAGE" "$APPIMAGE.zsync" SHA256SUMS "$output/"
    printf 'AppImage: %s/%s\n' "$output" "$APPIMAGE"
