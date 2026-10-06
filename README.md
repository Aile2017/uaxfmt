# uaxfmt

UAX #14(Unicode の改行規則)に基づき、日本語の禁則処理(ぶら下げ・追い出し)を行う
Windows 用のテキスト整形フィルタです。Vim プラグイン [vim-jp/autofmt](https://github.com/vim-jp/autofmt)
と同等の機能を、単体の exe として提供します。

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

Rust(MSVC ツールチェーン)が必要です。

```
cargo build --release
```

`target\release\uaxfmt.exe` ができます。C ランタイムを静的リンクするので、DLL を同梱せずに配布できます。

Git Bash からビルドすると、Git 付属の `link` コマンドが MSVC のリンカより先に見つかって
リンクに失敗します。PowerShell かコマンドプロンプトからビルドしてください。

## ライセンス

未定
