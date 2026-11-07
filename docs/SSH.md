# SSH key access for Linux hosts

Back to the [README](../README.md).

The remote probe and connection are non-interactive. Set up key-based SSH from
the client to every Linux host **before** using `niruremote peers --refresh`:

```sh
# On the client, if you do not already have an SSH key:
ssh-keygen -t ed25519 -C "$USER@$(hostname)"

# Copy the public key to the host:
ssh-copy-id user@HOST

# Verify that a non-interactive command works:
ssh HOST niruremote probe --json
```

Use the **Tailscale DNS name that `niruremote peers` shows** (for example
`desktop.tailnet.ts.net`) or the Tailscale IPv4 address (`tailscale ip -4`
on the host). NIRUREMOTE probes that exact host name, so a key trusted only under
the short name `desktop` still produces `host-key-untrusted` under the FQDN.
Run `ssh-copy-id` and `ssh` against the FQDN once:

```sh
ssh-copy-id user@desktop.tailnet.ts.net
ssh user@desktop.tailnet.ts.net niruremote probe --json
```

Verify the host-key fingerprint the first time. If `ssh HOST niruremote probe
--json` fails, fix SSH first; NIRUREMOTE will report `host-key-untrusted`,
`ssh-denied` or `ssh-timeout` rather than prompting.

Tailscale SSH is **not** required or assumed. NIRUREMOTE uses the normal
OpenSSH server on the Tailscale address.

If your key is passphrase-protected, load it into an agent before using
NIRUREMOTE. The non-interactive probe uses `BatchMode=yes` and cannot prompt:

```sh
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519
ssh HOST niruremote probe --json
```

In a NIRUX graphical session, start the agent from the session (or use your
keyring) so the launcher and terminal inherit `SSH_AUTH_SOCK`.

NIRUREMOTE also looks for a working agent itself: it checks the current
`SSH_AUTH_SOCK`, then `gcr/ssh`, `openssh_agent`, `keyring/ssh` and `ssh-agent`,
and uses the first socket that actually has identities. This handles the common
NIRUX/GNOME Keyring case where the session points at an empty
`/run/user/UID/openssh_agent` while the unlocked key is in
`/run/user/UID/gcr/ssh`.
