# C2 Agent — Ollama-Directed Flipper BLE Attack Controller

Python agent that connects to a Flipper MCP relay and uses Ollama to autonomously direct BLE HID injection and beacon spoofing attacks through a SubGHz C2 channel.

**For authorized security research only.**

## Prerequisites

- Python 3.8+
- Ollama running locally with a tool-calling model (llama3.1, qwen2.5, mistral)
- Flipper MCP with C2 module (ESP32 WiFi dev board)
- C2 Client FAP running on a second Flipper Zero

## Setup

```bash
pip install -r requirements.txt
ollama pull llama3.1
```

## Usage

### Interactive Mode

```bash
python c2_agent.py --mcp http://192.168.1.100:8080/mcp
```

Commands:
| Command | Description |
|---------|-------------|
| `radio start [freq]` | Start C2 SubGHz radio (default 433.92 MHz) |
| `ping` | Ping client Flipper |
| `hid start [name]` | Start BLE HID on client |
| `hid type <text>` | Type text on target |
| `hid press <key>` | Press key combo (e.g., `GUI+r`) |
| `hid stop` | Stop BLE HID |
| `beacon start <hex>` | Start BLE beacon |
| `beacon stop` | Stop beacon |
| `stop` | Stop all attacks |
| `status` | Get C2 status |
| `auto <objective>` | Switch to autonomous mode |
| `quit` | Exit |

### Autonomous Mode

```bash
python c2_agent.py --mcp http://192.168.1.100:8080/mcp --auto "Open notepad and type hello"
```

Ollama plans and executes the attack sequence using tool-calling.

### Options

| Flag | Description |
|------|-------------|
| `--mcp URL` | MCP endpoint (required) |
| `--ollama-url URL` | Ollama API (default: http://localhost:11434) |
| `--model NAME` | Ollama model (default: llama3.1) |
| `--auto OBJECTIVE` | Autonomous mode objective |
| `--timeout SECS` | MCP request timeout (default: 60) |

## Architecture

```
c2_agent.py ──HTTP──> ESP32 MCP ──UART──> Flipper FAP ──SubGHz──> C2 Client ──BLE──> Target
                  ↑
            Ollama LLM
```
