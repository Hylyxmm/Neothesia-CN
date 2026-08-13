![Neothesia Baner](https://github.com/user-attachments/assets/383438e5-80cd-49d2-af30-85afe5d79c6b)


# Neothesia（中文版）

本项目是基于开源项目 **Neothesia** 的汉化与功能改编版本。

## 项目来源

- **上游项目**：[PolyMeilex/Neothesia](https://github.com/PolyMeilex/Neothesia)
- **原作者**：PolyMeilex 及社区贡献者
- **许可证**：GNU General Public License v3.0（GPL-3.0）

Neothesia 是一个用 Rust 编写的跨平台 MIDI 可视化工具（类似开源版的 Synthesia），把 MIDI 文件中的音符显示为虚拟钢琴上落下的彩色方块，帮助用户学习钢琴。

## 改造内容

相对上游版本，本分支主要做了：

- **中文本地化**：界面与说明文档中文化。
- **新增五线谱（staff）预览**：在瀑布画面中加入滚动的大谱表预览，符干跟随声部。
- **完善记谱**：支持符尾连接（beaming）、装饰音（倚音）、八度移位标记（8va/8vb 等）。
- **拍号适配**：小节线按实际拍号计算，兼容 3/4、6/8、2/2 等拍号。

## 改造执行

本汉化与功能改造工作由 AI 编程助手完成：

- **Claude Code**（Anthropic）
- **ZCode + GLM-5.2**

人工主导需求、验收与决策。

## 构建产物

`dist/` 目录下提供已编译的可执行文件（仅供便利，非上游官方构建）：

- `dist/linux/neothesia` —— Linux x86-64
- `dist/windows/neothesia.exe` —— Windows x86-64

如需自行构建，在对应平台执行 `cargo build --release`。

---

# Neothesia

Neothesia is a cross-platform MIDI visualizer build in Rust.
It helps people to quickly learn how to play piano.
It takes music notes from a MIDI file as an input and displays them as colorful falling blocks on a virtual piano.

Opensource Synthesia was abandoned in favour of [closed source commercial project](https://www.synthesiagame.com/)  
The goal of this project is to bring Opensource Synthesia back to life, and make it look and work as good (or even better) than commercial Synthesia.

If you have any questions, feel free to join my Discord

[<img alt="Discord" src="https://img.shields.io/discord/273176778946641920?logo=discord&style=for-the-badge&color=%23a051ee">](https://discord.gg/sgeZuVA)

## Screenshots

![image](https://github.com/PolyMeilex/Neothesia/assets/20758186/65483bab-0b74-4fd4-90b1-fdd00508b676)

[![Video](https://github.com/PolyMeilex/Neothesia/assets/20758186/dc564433-aade-4430-b137-5f90000ae9e0)](https://youtu.be/ReE9nVuMCSE)

|![settings](https://github.com/PolyMeilex/Neothesia/assets/20758186/e38642e2-6118-4931-9964-a1df27a36db9)|![track selection](https://github.com/PolyMeilex/Neothesia/assets/20758186/2309d970-0234-45ff-a9f4-105ff08514af)|
|--|--|

[Video](https://youtu.be/ReE9nVuMCSE)

## Download

<a href="https://flathub.org/apps/details/com.github.polymeilex.neothesia"><img width="240" alt="Download on Flathub" src="https://flathub.org/assets/badges/flathub-badge-en.png"/></a>

Arch Linux (**Unofficial AUR** built from source, maintained by @zayn7lie): <https://aur.archlinux.org/packages/neothesia>

All binary releases:
[https://github.com/PolyMeilex/Neothesia/releases](https://github.com/PolyMeilex/Neothesia/releases)

## FAQ

- [FAQ](https://polymeilex.github.io/Neothesia/pages/installation.html)
- [Video encoding](https://polymeilex.github.io/Neothesia/pages/video-encoding.html)

## Thanks to

- [WGPU](https://wgpu.rs/)
- [Linthesia](https://github.com/linthesia/linthesia)
- [Synthesia](https://github.com/johndpope/pianogame)
