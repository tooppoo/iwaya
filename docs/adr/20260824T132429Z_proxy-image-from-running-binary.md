# Assemble the Proxy Image from the Running Binary and Its Loaded Objects

- Status: Accepted
- Created: 2026-08-24T13:24:29Z

## Context

[Add Proxy-Backed Secret Delivery](20260820T162206Z_proxy-backed-secret-delivery.md) requires the proxy sidecar image to be iwaya-owned, keyed to the iwaya version, built lazily on first proxy use, and built without registry or network access. It leaves open what actually goes into the image: `FROM scratch` rules out every base image, so all executable and filesystem material must come from somewhere local and trusted, and the proxy itself is not a separately distributed executable but the iwaya binary in its `proxy` execution mode.

The unresolved question is how a locally built image obtains a working `/iwaya` when the running binary is dynamically linked, which is what `cargo build` produces for the default `x86_64-unknown-linux-gnu` target: the binary alone cannot start in an empty root filesystem because its ELF interpreter and shared libraries are absent. This needs an ADR because the answer decides what host material enters the image, whether proxy-backed delivery imposes build-target requirements on iwaya itself, and adds a dependency for the image-identity digest.

Related issues:

- #31, #41

Related ADRs:

- [Add Proxy-Backed Secret Delivery](20260820T162206Z_proxy-backed-secret-delivery.md) fixes the constraints this ADR works inside — embedded recipe, lazy local build, no network — and already rejected a registry-published image.
- [Implement iwaya in Rust](20260808T171732Z_implement-iwaya-in-rust.md) sets the dependency posture the `sha2` addition follows: add a dependency when a concrete need appears, and prefer the standard, smaller one.

## Decision

The image is `FROM scratch` plus a `rootfs/` assembled at build time from three sources: the running iwaya binary copied to `/iwaya`, the ELF program interpreter the binary names in its `PT_INTERP` header, and every file-backed shared object currently mapped into the running process according to `/proc/self/maps`, each copied to the absolute path it was loaded from. The entrypoint is `["/iwaya", "proxy"]`. A statically linked binary has no interpreter and no mapped objects, so its `rootfs/` naturally degenerates to the binary alone; nothing about the recipe assumes either linkage.

The loaded-object list is used instead of a predicted dependency list because it is the closure the dynamic linker actually resolved for this exact binary on this exact host: copying those files to the same paths reproduces inside the image the lookup that already succeeded outside it. TLS needs no extra image material because iwaya's HTTP client compiles its trust roots in (`ureq` with `rustls` and `webpki-roots`), and name resolution needs none because the container runtime provides `resolv.conf` and glibc 2.34+ resolves DNS without separate NSS modules.

The image tag is `iwaya-proxy:v<version>-<digest>`, where the digest is the first 12 hex characters of a SHA-256 (via the `sha2` crate) over the embedded Dockerfile and every material file's destination path and content. A tag lookup (`<runtime> image inspect`) decides between reuse and build, so an unchanged binary reuses the image and a locally rebuilt binary — same version, different bytes — never silently reuses a stale one. The build context is assembled in a temporary directory that is removed whether the build succeeds or fails.

## Alternatives Considered

### Requiring a statically linked iwaya for proxy use

Shipping only the binary and refusing dynamically linked builds keeps the image purely iwaya-owned. It was rejected because iwaya cannot produce a static binary of itself at proxy time — that would need a toolchain and network — so the requirement would fall on whoever built the binary, breaking proxy-backed delivery for every default `cargo build` and for this project's own development environment, where no musl target is installed. Bundling the loaded objects costs nothing when the binary is static, so the chosen recipe keeps a future static release binary as the clean case without making it a prerequisite.

### A base image providing libc

`FROM debian` or `FROM alpine` would provide the runtime libraries without copying host files. It was rejected because pulling a base image is exactly the registry and network dependency the parent ADR forbids, and a pre-pulled base image would reintroduce it as a hidden setup step.

### Discovering dependencies with `ldd` or a fixed library list

Running `ldd` on the binary spawns a subprocess and parses distribution-specific output; hard-coding `libc`/`libgcc` paths breaks on the next distribution layout or added dependency. `/proc/self/maps` is already the kernel's answer for this process, costs no subprocess, and automatically tracks whatever the linker loaded — including objects a future dependency pulls in.

## Consequences

### Positive Consequences

- Proxy-backed delivery works with any iwaya binary as-is: no musl target, no toolchain, no registry, and no network are required at build time.
- A rebuilt binary changes the digest and forces a rebuild, so a stale proxy image cannot be silently reused within one version.
- The recipe stays embedded and the context is temporary, so nothing user-editable or persistent is created in the project.

### Negative Consequences

- Host libraries (the interpreter, `libc`, `libgcc`) are copied into the image, so the image is host-derived local material rather than purely iwaya-owned bytes, and it is only guaranteed to run on the architecture that built it — acceptable because the sidecar always runs on the same host as the supervisor that built it.
- Name resolution inside the image relies on glibc 2.34+ having merged its NSS modules into `libc`; on an older glibc host, DNS inside the sidecar may fail even though the same binary resolves names on the host. The sidecar wiring's end-to-end tests (#42, #43) are where this would surface.

### Neutral Consequences

- `sha2` joins the dependency tree for the digest; it is the standard pure-Rust SHA-256 implementation and follows the established add-when-needed posture.
- Image garbage collection remains out of scope, as the parent ADR already records; digest-suffixed tags accumulate one image per distinct local build.
