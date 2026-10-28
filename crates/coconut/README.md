# coconut

Born-at-mode private file and directory creation, plus fd-verified mode
re-assertion. Published on crates.io as `coconut-fs` (the bare name was taken);
the library target stays `coconut`.

Three primitives for secrets-adjacent filesystem paths: `ensure_dir_mode`
creates a directory or repairs an existing one, `create_exclusive` creates a
new file, and `open_verified` opens an existing file and verifies its mode.
Each closes two gaps:

- No umask-default window. The mode is passed into the create syscall itself
  (`DirBuilder::mode` / `OpenOptions::mode`), so the inode is born no wider
  than the requested mode; an `fchmod` on the fresh fd then repairs any
  `& ~umask` narrowing back to exactly the requested mode.
- No path race on re-assertion. Verification opens with `O_NOFOLLOW` (a
  planted symlink fails to open), stats the fd, and repairs drift with
  `fchmod` on that same fd. No second path-based stat or chmod runs that a
  concurrent rename could redirect. The whole low-12 permission word
  (`& 0o7777`) is compared, so a drifted setuid/setgid/sticky bit counts as
  drift and is repaired away.

`open_verified` returns the verified fd so callers read through it; re-opening
by path after the check would reintroduce the window the fd discipline closes.

Mode enforcement is Unix-only (`cfg(unix)`). On other targets the functions
fall back to the plain std operations (`create_dir_all` / `create_new` /
`open`) and enforce no mode. The one dependency is `libc`, on Unix targets.

Extracted from the secrets handling of the fruit-basket appliance OS and its
reference tenant, where these primitives own every private key, cookie key,
and CA file on disk.

## License

MIT OR Apache-2.0.
