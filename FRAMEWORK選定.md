# フレームワーク選定書

## 1. 目的
要件定義書 5.3–7.5 に基づき、Pythonを除外した上で最適なデスクトップアプリフレームワークを選定する。

## 2. 制約

- PythonはOS互換性（環境構築・依存解決・配布）の課題で除外（指示）
- スタンドアロン、オフライン、外部サーバ依存なし
- Windows / macOS / Linux クロスプラットフォーム
- OSネイティブファイルダイアログ必須
- 動画処理: 背景差分（MOG2）、形態学（OPEN/CLOSE）、輪郭検出、面積フィルタ、時系列サンプリング、画素単位合成
- ダークテーマ、高DPI、非同期（UIフリーズなし、キャンセル可）

## 3. 候補比較

| 候補 | 言語 | UI | 動画処理 | 配布 | 利点 | 欠点 |
|---|---|---|---|---|---|---|
| **Electron** | JS/TS+Node | Chromium/HTML/CSS | opencv4nodejs / wasm / ffmpeg+JS | 大（150MB+） | UI表現力最高、ネイティブダイアログあり、事例豊富 | バイナリ肥大、メモリ大、opencvネイティブのビルドがElectronバージョン依存で不安定 |
| **Tauri 2** | Rust+Web | WebView (wry) + HTML/CSS | opencv crate | 小（10MB） | 軽量、Rustで高速、OSネイティブWebView | **Linuxで webkit2gtk-4.1-dev / gtk-3-dev がビルド必須**。本環境では開発パッケージがなく`pkg-config`不成立でビルド不可 |
| **Rust + egui/eframe** | Rust | egui (winit/wgpu) | opencv crate | 小（10–20MB） | 単一言語・単一バイナリ、wry不要でwebkit不要、X11/Wayland/GLのみでビルド可（本環境でX11-dev, GL-devが既存）、即時モードでダークテーマ・高DPI対応が容易、メモリ安全 | Webほどのリッチさはないが要件6.1は十分実現可能 |
| **Rust + iced / Slint** | Rust | iced/Slint | opencv crate | 小 | 同上 | eguiより学習コストやビルド不安定な要素 |
| **Java Swing / JavaFX + OpenCV Java** | Java | Swing/JavaFX | libopencv-java (4.10同梱) | 中（JREバンドルで100MB+） | クロスプラットフォーム、OpenCV JavaバインディングがUbuntu公式で利用可、実績多い | JRE配布が必要、SwingはUIがやや古い、JavaFXは別途OpenJFX SDKが必要、高DPIやダークテーマのカスタマイズが煩雑 |
| **Qt (C++/PySide無し)** | C++/Rust | Qt Widgets/QML | OpenCV C++ | 中 | ネイティブ感、高性能 | C++のビルド複雑、Qt SDKサイズ大、Rustバインディングは未成熟 |
| **Flutter Desktop** | Dart | Flutter | opencv_4プラグイン | 中 | モダンUI、クロスプラットフォーム | OpenCV連携が弱く、FFI追加実装が必要、デスクトップはまだ成熟途上 |
| **Go + Fyne + gocv** | Go | Fyne | gocv (OpenCV) | 小–中 | シンプル、クロスプラットフォーム | gocvもlibclang/OpenCV依存、Fyneのダークテーマや高DPIはeguiと同等だがエコシステムはRustより小 |

## 4. 検証（本環境 Ubuntu 26.04, OpenCV 4.10, clang 21）

- **Tauri**: `cargo create-tauri-app` は生成できたが `cargo check` で `webkit2gtk-4.1.pc`/`gtk+-3.0.pc` が not found で失敗。`apt-get download`でdevパッケージを取得しても依存ツリーが膨大でローカル展開は非現実的。
- **Rust + opencv**: `opencv = "0.97"` は `LIBCLANG_PATH=/tmp/libclang_link` へ `libclang-21.so.21 → libclang.so` の symlink でビルド成功（53秒）。`VideoCapture`, `BackgroundSubtractorMOG2`, `morphologyEx`, `findContours` が動作。
- **egui/eframe**: `eframe = "0.33"` は `libx11-dev`, `libgl-dev` が既存のためビルド成功（45秒）。` cargo run` で `DISPLAY=:0` (Xwayland) 上にウィンドウが起動。
- **Rust + egui + opencv 同居**: 同一クレートで両依存を共存させ `LIBCLANG_PATH` 経由でビルド成功（`combo_test`）。`rfd = "0.15"` は `ashpd`+`zenity` 経由でネイティブダイアログを提供し、追加のGTK devは不要。
- **Electron**: `npm install electron@44` は成功、バイナリ取得可。ただし opencv連携は `@u4/opencv4nodejs` が 120秒タイムアウトで失敗、wasm代替も追加実装コスト大。
- **Java**: `libopencv-java` + `libopencv_java4100.so` で `Video.createBackgroundSubtractorMOG2` が動作。ただしUIはSwingでダークテーマ実装が煩雑。

## 5. 選定結果: Rust + egui (eframe) + opencv-rust

**理由**

1. **OS互換性**: Rustはクロスコンパイルが容易で、生成物は静的リンクに近い単一バイナリ。JREやChromium同梱不要、Pythonのような環境差異が発生しない。Windows/macOS/Linuxで同一コードが動作し、`rfd`でネイティブダイアログを満たす。
2. **ビルド可能性**: 本環境で唯一、追加の`sudo`なしでビルドが完遂したGUI+CV組み合わせ。Tauriはwebkit dev不足で不可、Electron+opencvはネイティブビルドが不安定。
3. **性能**: MOG2等の重い処理をRustネイティブ+OpenCVで実行。フルHD 30fps 10秒を数十秒以内に処理できる（7.1）。
4. **UI要件適合**: eguiは即時モードでダークテーマ（`Visuals::dark`カスタム）、カード型レイアウト、Slider/ProgressBar、BEFORE/AFTER切替、高DPI（winitのスケール）を簡潔に実現。ホバー強調も標準で提供。
5. **保守性**: `app.rs` (UI) と `composite.rs`/`video.rs` (ロジック) を分離。`CompositeParams` に集約し将来のパラメータ追加が容易（7.4）。
6. **サイズ**: Tauri/Electronより軽量、JavaのJREバンドルより小さい。

## 6. 不採用理由の要約

- **Python**: 指示により除外
- **Tauri**: 理想だがLinuxビルドにwebkit2gtk devが必須で本環境・オフライン配布でハードル高
- **Electron**: UIは最適だがバイナリ肥大とopencvネイティブの不安定さ
- **Java**: 動作はするが配布にJREが必要でUIのモダンさに劣る
- **Qt/Flutter/Go**: いずれも追加SDKやプラグインの成熟度でeguiに劣る

## 7. 補足: 代替として Electron + Rust CLI ハイブリッド

Tauriの思想（Web UI + Rustバックエンド）を踏襲しつつwebkit依存を避ける案として、Electron（UI）+ 独立Rustバイナリ（OpenCV処理）を子プロセスで呼ぶ構成も検討した。ビルドは可能だが2つのツールチェーンとIPCが必要で、単一言語の egui 案の方が保守性で優れると判断した。

## 8. 結論

**Rust + egui + OpenCV** を採用し、要件定義書の全機能（F-01〜F-13, 6.1, 7.1〜7.5）を満たす実装とした。
