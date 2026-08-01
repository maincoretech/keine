# 行业引擎参考

## 商业引擎

| 引擎 | 语言 | 体积 | 特点 |
|------|------|------|------|
| SiglusEngine | C++ | ~5MB | Key 全系列，字节码 VM |
| Kirikiri 2/Z | C++ | ~2MB | 柚子社，TJS2 脚本 |
| YU-RIS | C++ | ~1.5MB | 延迟命令队列 |
| NScripter | C | ~500KB | 寒蝉/月姬，极致轻量 |

## 开源引擎

| 引擎 | 语言 | 渲染 | 体积 |
|------|------|------|------|
| Ren'Py | Python+C | OpenGL/SDL | ~70MB |
| Suika2 | C | OpenGL 1.x | ~300KB |
| WebGAL | TS/React | PIXI.js | ~5MB(JS) |
| Ayaka | Rust+Tauri | PIXI.js(WebView) | ~15MB |

## 关键借鉴

| 引擎 | 借鉴 | 用于 |
|------|------|------|
| YU-RIS | 命令队列 + batch 合并 | 渲染层 2-5 draw calls |
| Bevy | Plugin + Schedule + 双世界 | 核心 ECS 架构 |
| WebGAL | 编辑器预览协议 | keine dev 实时预览 |
| Ayaka | Tauri + WASM 插件 | 桌面壳 + 扩展系统 |
| Ren'Py | 差分 rollback | 存档回退 |
| hexz_k | .hxz 加密归档 | 资源打包分发 |
