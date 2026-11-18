# FeliCa Reader / Writer

`felica-rs` が対応する Sony PaSoRi 系リーダーから FeliCa カードの情報、
サービスコード、認証不要ブロックを読み取り、認証不要ブロックへ任意の
16バイトデータを書き込むネイティブGUIアプリです。

## 対応ハードウェア

`felica` クレートの USB ドライバーが対応する端末を使用できます。

- Sony RC-S380 / RC-S634
- Sony RC-S300
- Sony RC-S320
- Sony RC-S330 / RC-S360 / RC-S370

## 起動

```sh
cargo run
```

1. 「リーダー接続」を押します。
2. カードをリーダーに置き、「カード検出 / 一覧」を押します。
3. サービスコードを選択し、開始ブロックと件数を指定して「読み取り」を押します。
4. 書込み時は「16バイト（32桁）の16進データ」を入力し、「書き込む」を押します。

認証が必要なサービスへの読み書きは、現在のライブラリAPI範囲では
認証不要ブロックのみ対応しています。カードやサービスの仕様により、
読み書きできない場合があります。

## ビルド

### GitHub Actions

`v*` タグを pushした場合のみ、以下をネイティブ環境でビルドします。

- Linux x86_64
- Windows x86_64
- macOS x86_64
- macOS arm64

通常の push や pull request ではビルドを実行しません。タグ付け時のみ、
各成果物をGitHub Releaseへ添付します。

### ローカルビルド

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

LinuxでUSBリーダーを使用する場合は、libusbとudevへのアクセス権限に加え、
libusbの開発パッケージが必要です。
