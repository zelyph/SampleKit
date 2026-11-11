"""samplekit-migrate: a project of an earlier SampleKit, carried into a copy
this version reads."""

# The version this tool writes for: the samplekit installed beside it must be
# this one. A test holds it to Cargo.toml's.
FOR_VERSION = "1.0.0-rc.3"

# Each version published, and the reader that understands it: the four 0.x
# wrote the same files.
PUBLISHED = {
    "0.1.0": "0.x",
    "0.1.1": "0.x",
    "0.1.2": "0.x",
    "0.2.0": "0.x",
    "1.0.0-rc.1": "1.0.0-rc.1",
}
