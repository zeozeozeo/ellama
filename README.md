# 🦙 Ellama [![Ellama Stars](https://img.shields.io/github/stars/zeozeozeo/ellama.svg)](https://github.com/zeozeozeo/ellama)
 
Ellama is a friendly interface to chat with a local or remote [Ollama](https://ollama.com/) instance.

![Ellama, a friendly Ollama interface, running LLaVA](/media/pokey.png)

# 🦙 Features

* **Chat History**: create, delete and edit model settings per-chat.
* **Multimodality**: easily use vision capabilities of any multimodal model, such as [LLaVA](https://ollama.com/library/llava).
* **Ollama**: no need to install new inference engines, connect to a regular [Ollama](https://ollama.com/) instance instead.
* **Resource Efficient**: minimal RAM and CPU usage.
* **Free**: no need to buy any subscriptions or servers, just fire up a local Ollama instance.

# 🦙 Quickstart

1. Download the latest Ellama release from the [Releases](https://github.com/zeozeozeo/ellama/releases) page.
   * or, if you have `cargo` installed:
        ```bash
        $ cargo install ellama
        ```
2. In the Settings ⚙️ tab, change the Ollama host if needed (default is `http://127.0.0.1:11434`)
3. In the same tab, select a model that will be used for new chats by default. Ellama will try to select the best model on the first run.
4. Close the Settings tab, create a new chat by pressing the "➕ New Chat" button, and start chatting!
5. To add images, click the ➕ button next to the text field, drag them onto Ellama's window, or paste them from your clipboard.

# 🦙 Gallery

https://github.com/zeozeozeo/ellama/assets/108888572/c7fe07b8-1b46-47cc-bae1-2b7e087d5482

![Ellama's greeting screen](/media/funfact.png)

![LLaVA counting people, in Ellama](/media/countppl.png)

![Ellama's settings panel](/media/setthings.png)

![Ellama's chat edit panel](/media/chatedit.png)

# License

MIT OR Apache-2.0
