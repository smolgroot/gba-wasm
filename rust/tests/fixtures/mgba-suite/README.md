# mGBA suite fixture

`suite.gba` is built from [mgba-emu/suite](https://github.com/mgba-emu/suite), commit `e6942030d25ffe3ba76c72b73a86da073ec857cc`, distributed under the MIT license in `LICENSE`. The project publishes no prebuilt release, so the binary was built, unmodified, with the devkitARM image that its `docker-build.sh` uses:

```sh
git clone https://github.com/mgba-emu/suite && cd suite
git checkout e6942030d25ffe3ba76c72b73a86da073ec857cc
docker run --rm -v "$PWD":/root/suite -w /root/suite devkitpro/devkitarm make
```

SHA-256 of the build vendored here: `8cf68cd31c5468a70aca8a1c5b63dc372fe755c40b446cf6aa6d0fc6e3a1f035`. A different devkitARM release can produce a different binary, so the baselines in `rust/tests/mgba_suite.rs` apply to this file. The harness is described in `doc/rust-core/testing-strategy.md`.
