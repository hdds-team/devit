# devit-studio

**Local-first IDE with AI ghost cursor** 👻

> Watch AI write code in real-time. Accept or reject with a keystroke.

![Ghost Cursor Demo](assets/demo.gif)
<!-- TODO: Record 15s GIF showing ghost cursor in action -->

---

## ✨ Features

**🤖 Ghost Cursor** — AI types code live at your cursor position. Semi-transparent preview, you stay in control.

**🏠 Local-First** — Runs with Ollama, LM Studio, llama.cpp. Your code never leaves your machine.

**⚡ Lightweight** — ~300KB frontend (CodeMirror 6 + Svelte). No Electron bloat.

**🔌 LSP Support** — rust-analyzer, pylsp, typescript-language-server, gopls, clangd.

**🌙 Dark Theme** — Easy on the eyes. That's it for v1.

---

## 🎬 Quick Demo

```
1. Open a file
2. Position cursor where you want code
3. Ask in chat: "Add error handling here"
4. Watch ghost cursor type the solution
5. Press Tab to accept, Escape to reject
```

---

## 🚀 Getting Started

### Prerequisites

- Rust 1.75+
- Node.js 20+
- [Ollama](https://ollama.ai) running locally
- Tauri CLI: `cargo install tauri-cli`

### Build & Run

```bash
# Clone
git clone https://git.hdds.io/hdds/devit.git
cd devit

# Frontend
cd studio-ui && npm install && cd ..

# Run
cd crates/studio && cargo tauri dev
```

### First Launch

1. Start Ollama: `ollama serve`
2. Pull a model: `ollama pull codellama` or `ollama pull deepseek-coder`
3. Launch devit-studio
4. Open a folder, start coding with AI

---

## ⌨️ Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Accept ghost edit |
| `Escape` | Reject ghost edit |
| `Ctrl+Enter` | Send chat message |
| `Ctrl+S` | Save file |

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Tauri WebView (Svelte)                   │
│  ┌───────────┐  ┌───────────┐  ┌─────────────────────────┐ │
│  │ CodeMirror│  │   Chat    │  │      Explorer           │ │
│  │  + Ghost  │  │ (Stream)  │  │    (File tree)          │ │
│  └─────┬─────┘  └─────┬─────┘  └─────────────────────────┘ │
├────────┴──────────────┴─────────────────────────────────────┤
│                      Rust Backend                           │
│  ┌───────────┐  ┌───────────┐  ┌─────────────────────────┐ │
│  │    LSP    │  │    LLM    │  │      Workspace          │ │
│  │ (JSON-RPC)│  │ (Ollama)  │  │   (Files, Git)          │ │
│  └───────────┘  └───────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

---

## 🗺️ Roadmap

### v0.1 — Now
- [x] CodeMirror 6 editor
- [x] Ollama chat streaming
- [x] Ghost cursor
- [x] LSP basics

### v0.2 — Next
- [ ] File attachments (images, PDF)
- [ ] More LLM providers (LM Studio, llama.cpp)
- [ ] Git status in explorer

### v0.3 — Later
- [ ] Dockable panels (drag & drop)
- [ ] Custom themes
- [ ] Plugin system

---

## 🤝 Contributing

We love contributions! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

**Good first issues:**
- [ ] Add syntax highlighting for more languages
- [ ] Improve ghost cursor animations
- [ ] Add keyboard shortcuts

```bash
# Dev workflow
cd studio-ui && npm run dev      # Frontend hot-reload
cd crates/studio && cargo watch  # Backend rebuild
```

---

## 💬 Community

- **Issues**: Bug reports & feature requests
- **Discussions**: Questions & ideas
- **PRs**: Code contributions welcome

---

## 📄 License

MIT — Same as devit project.

---

<p align="center">
  <b>Built for developers who want AI assistance without the cloud lock-in.</b>
  <br><br>
  ⭐ Star if you find this useful!
</p>
