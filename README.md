# uaxfmt

UAX #14(Unicode の改行規則)に基づき、日本語の禁則処理(ぶら下げ・追い出し)を行う
Windows 用のテキスト整形フィルタです。

詳しい仕様は [docs/SPEC.md](docs/SPEC.md) を参照してください。

## 使い方

```
uaxfmt -i in.txt -o out.txt      # ファイルを整形してファイルへ
type in.txt | uaxfmt -w 72       # 標準入力から標準出力へ(72 桁で折り返し)
uaxfmt -h                        # ヘルプ
```

## 設定ファイル

exe と同じフォルダの `<exe のベース名>.toml`(`uaxfmt.exe` なら `uaxfmt.toml`)を読み込みます。
次のコマンドで、全項目と説明コメント入りの設定ファイルを作れます。

```
uaxfmt -p -o C:\tools\uaxfmt.toml
```

## 文字コード

入力の文字コード(UTF-8、UTF-16、CP932、EUC-JP、ISO-2022-JP)を自動で判定し、
出力も同じ文字コードで書きます。判定が外れる場合は、設定ファイルの `encoding` で指定してください。

### パイプの先での文字化けについて

出力をコンソールに直接表示する場合は、コンソールのコードページに関係なく正しく表示されます。
一方、パイプで他のコマンドに渡す場合は、受け取る側が出力をどう解釈するかによって文字化けすることがあります。

- cmd.exe で UTF-8 のファイルを整形して `more` などに渡すと、CP932 として解釈されて文字化けします。
- PowerShell で uaxfmt の出力をコマンドレット(`Select-String` など)に渡す場合は、
  事前に次の設定をしてください。

  ```powershell
  [Console]::OutputEncoding = [Text.Encoding]::UTF8
  ```

## ビルド

Windows x64 上で、次のツールが必要です。

- Rust 1.88 以降(MSVC ツールチェーン: `stable-x86_64-pc-windows-msvc`)
- Visual Studio 2022 Build Tools の「C++ によるデスクトップ開発」
  (MSVC x64/x86 ビルドツールと Windows SDK)

未導入の場合は、PowerShell から次のコマンドでインストールできます。
Build Tools のインストール時は管理者権限の確認が表示される場合があります。

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --source winget --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --norestart"
winget install --id Rustlang.Rustup --exact --source winget
```

インストール後は PowerShell を開き直して、ツールチェーンを確認してください。

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustc --version
cargo --version
```

リポジトリのルートでビルドします。初回は依存クレートのダウンロードにネットワーク接続が必要です。

```
cargo build --release --locked
```

`target\release\uaxfmt.exe` ができます。C ランタイムを静的リンクするので、DLL を同梱せずに配布できます。

単体テストと実行確認:

```powershell
cargo test --locked
.\target\release\uaxfmt.exe -v
.\target\release\uaxfmt.exe -i in.txt -w 72
```

Git Bash からビルドすると、Git 付属の `link` コマンドが MSVC のリンカより先に見つかって
リンクに失敗します。PowerShell かコマンドプロンプトからビルドしてください。

## ライセンス

未定
