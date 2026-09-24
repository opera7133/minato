# macOS

```sh
rustup target add --toolchain 1.93.0 aarch64-apple-darwin x86_64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=14.0 cargo +1.93.0 build --locked --release --lib --target aarch64-apple-darwin
```

`target/aarch64-apple-darwin/release/libminato.dylib` を `ghost/master/` に置き、`descript.txt` に追加します。

```text
shiori.macos,libminato.dylib
```

- `loadu` / `load` / `request` / `unload` を公開します。文字列はUTF-8、長さは符号付き32ビット整数です。入力はライブラリが `free` し、応答は呼び出し側が `free` します。
- 同じプロセス内のライブラリへの二重ロードは失敗します。Utataneはゴーストごとにホストプロセスを分けて複数起動します。
- SAORIはmacOS用ライブラリを使用します。台本の `echo.dll` は同じ場所の `echo.dylib` を読み込みます。SAORIの文字列・メモリ所有権も上記と同じです。
- 保存先はゴーストの `save.json` です。書き込みに失敗した場合、`unload` は0を返し、再試行できるように状態を保持します。

検証は `cargo +1.93.0 test --locked` で実行します。Windows版のShift_JIS応答に関する例には、UTF-8版の期待値を併記しています。

再配布時は `LICENSE` と `THIRD-PARTY-LICENSES.toml` を含めてください。
