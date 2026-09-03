# FRAME MOTION STUDIO

動画から動体の残像を1枚に合成するデスクトップアプリケーション。  
スポーツ解析・ロボット軌跡確認などで、動きの経時的変化を1枚で俯瞰する。

> 要件定義書: `要件定義書.md` 準拠

## フレームワーク選定

詳細は `FRAMEWORK選定.md` を参照。  
**選定: Rust + egui (eframe) + OpenCV (opencv-rust)**  
- PythonはOS間の環境差異・依存解決・配布の互換性課題があるため除外
- Rustはシングルバイナリ・メモリ安全・高速・クロスプラットフォーム（Windows/macOS/Linux）
- egui/eframeは即時モードGUIでダークテーマ・高DPI対応が容易、WebView依存なし（Tauriのwebkit2gtk問題を回避）
- OpenCV 4.10はUbuntu公式パッケージ `libopencv-dev` / `libopencv-java` として提供され、Rustクレート `opencv` から MOG2・形態学・輪郭検出を直接利用可能
- ネイティブファイルダイアログは `rfd`（xdg-portal/zenity）でOSネイティブを実現

## 機能一覧（要件対応）

| 要件 | 実装 |
|---|---|
| F-01 動画選択 | `rfd::FileDialog` + D&D（eguiの`dropped_files`/`hovered_files`） |
| F-02 動画情報 | `VideoCapture::get(CAP_PROP_*)`で解像度/FPS/フレーム数/時間を`--`フォールバック付き表示 |
| F-03 保存先指定 | 保存ダイアログ、自動命名 `*_composite.png`, 拡張子補完、上書き確認ダイアログ |
| F-04 画像保存 | `imgcodecs::imwrite`、親ディレクトリ自動生成 |
| F-05 背景差分 | `createBackgroundSubtractorMOG2` → `morphologyEx(OPEN/CLOSE)` → `findContours` → `contourArea`閾値除去 |
| F-06 時系列サンプリング | `frame_idx/fps` で時刻算出、指定`interval_sec`ごとにサンプリング |
| F-07 合成 | 先頭フレームを背景とし、`core::copy_to`で動体画素のみ上書き、キャンセルは`AtomicBool`で中断 |
| F-08 パラメータ制御 | interval(0.05–1.0), minArea(10–2000), threshold(5–50), history(20–500) をSliderで制御、プリセット3種（きめ細かい/標準/あっさり）、詳細折りたたみ |
| F-09 プレビュー | 先頭フレームをRGBA変換→`ColorImage`→`TextureHandle`で表示、BEFORE/AFTER切替、アスペクト保持縮小 |
| F-10 結果オープン | `open::that`でOS標準ビューア起動（xdg-open/open/start） |
| F-11 進捗表示 | `ProgressBar`, %・現在秒、ヘッダステータス（READY/PROCESSING/DONE/CANCELLED/ERROR） |
| F-12 実行制御 | 合成ボタン/キャンセル排他、未選択警告 |
| F-13 エラーハンドリング | open失敗/FPS失敗/先頭フレーム失敗をダイアログ＋ERRORステータス |

画面構成は要件6.1準拠: ヘッダ / 左ペイン（動画・保存先・仕上がり） / 右ペイン（プレビュー・実行） / フッタ。ダークテーマ基調。

## 動作環境

- OS: Windows 10/11, macOS 13+, Ubuntu 22.04/24.04/26.04（開発確認: Ubuntu 26.04, OpenCV 4.10）
- 依存: OpenCV 4.x (`libopencv-dev`), libclang (`libclang-21`), Rust 1.75+, `ffmpeg`（動画デコードに必要だがOpenCVのvideoioが内包）
- オフライン動作、GPU不要

## ビルド & 実行

### 前提（Ubuntu/Debian）

```bash
# OpenCV とビルドツールは既に入っている想定（本リポジトリのCIでは /usr/lib/x86_64-linux-gnu/libopencv* が存在）
# 不足時のみ:
# sudo apt update
# sudo apt install libopencv-dev clang libclang-dev

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

本プロジェクトは `LIBCLANG_PATH` を `.cargo/config.toml` で `.libclang` に設定しています。  
初回ビルド前に以下でシンボリックリンクを作成（既に作成済みなら不要）:

```bash
mkdir -p .libclang
ln -sf /usr/lib/x86_64-linux-gnu/libclang-21.so.21 .libclang/libclang.so
# 別バージョンの場合:
# ln -sf /usr/lib/llvm-21/lib/libclang.so.1 .libclang/libclang.so
```

### デバッグ実行

```bash
cargo run
# または
cargo run --release
```

### リリースビルド

```bash
cargo build --release
# バイナリ: target/release/frame_motion_studio
./target/release/frame_motion_studio
```

### テスト動画生成（任意）

```bash
# 合成ロジックの手動テスト用に合成元動画を生成（Rustで生成）
# 既に /tmp/moving_box2.mp4 を生成済みだが再生成する場合:
cargo run --bin gen_vid  # 別途用意した生成バイナリがあれば
# または ffmpeg で:
ffmpeg -f lavfi -i testsrc=s=1280x720:r=30:d=10 -pix_fmt yuv420p /tmp/testsrc.mp4
```

## 使い方

1. 「参照」またはドラッグ＆ドロップで動画を選択 → ファイル名と動画情報が表示、BEFOREプレビューが更新
2. 保存先は自動で `入力名_composite.png` が設定（「変更」で別パス・PNG/JPEG選択可）
3. 仕上がりプリセット（きめ細かい/標準/あっさり）を選択、必要に応じて「詳細設定」で interval / 最小面積 / 感度 / 履歴を微調整
4. 「合成する」を押下 → 進捗バーと現在時刻が更新、キャンセル可
5. 完了後、プレビューがAFTERに切り替わり、「開く」でOSビューアで確認
6. 上書き時は確認ダイアログ、処理中のウィンドウCloseは確認ダイアログが表示

## ディレクトリ構成

```
frame_motion_studio/
  Cargo.toml          # 依存（eframe, egui, opencv, rfd, image, open）
  .cargo/config.toml  # LIBCLANG_PATH
  .libclang/          # libclang.so シンボリックリンク
  src/
    main.rs           # エントリ、ウィンドウ設定
    lib.rs            # ライブラリ再エクスポート（テスト用）
    app.rs            # UI（egui）・状態管理・非同期制御
    composite.rs      # 合成コア（MOG2, 形態学, 輪郭, 合成）
    video.rs          # 動画情報・先頭フレーム抽出・変換
  要件定義書.md
  FRAMEWORK選定.md
  README.md
```

## 非機能要件への対応

- **性能**: MOG2の`history/threshold`でチューニング、フルHD 10秒を数十秒以内に処理（Rustネイティブ + OpenCV）
- **互換性**: `rfd`がOSネイティブダイアログ、eguiが高DPI自動スケール、PNG/JPEGは`imgcodecs`が対応
- **信頼性**: キャンセル時は`AtomicBool`で即時中断し保存を行わない、終了時確認ダイアログ
- **保守性**: UI (`app.rs`) とロジック (`composite.rs`/`video.rs`) を分離、パラメータは`CompositeParams`に集約

## 今後の拡張（要件外）

- 複数動画の一括処理
- 透過度・色調整
- 検出方式選択（MOG2以外）
- タイムライン＋プレビュー再生

## ライセンス

MIT
