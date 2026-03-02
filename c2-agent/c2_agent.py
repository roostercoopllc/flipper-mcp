#!/usr/bin/env python3
"""
C2 Agent — Ollama-directed Flipper-to-Flipper BLE attack controller.

Connects to a Flipper MCP relay (ESP32) via HTTP and uses the C2 SubGHz
tools to command a remote client Flipper to execute BLE HID injection
and beacon spoofing attacks.

Supports two modes:
  - Interactive (default): Human types commands, Ollama helps plan
  - Autonomous (--auto): Ollama agent loop with tool-calling

Usage:
  python c2_agent.py --mcp http://192.168.1.100:8080/mcp
  python c2_agent.py --mcp http://localhost:9090/mcp --auto "Test BLE HID on target"

For authorized security research only.
"""

import argparse
import json
import sys
import time
from typing import Any, Optional

import requests
from rich.console import Console
from rich.panel import Panel
from rich.table import Table

console = Console()

# --- MCP Client ---

class MCPClient:
    """JSON-RPC client for the Flipper MCP server."""

    def __init__(self, url: str, timeout: int = 60):
        self.url = url
        self.timeout = timeout
        self._request_id = 0

    def call(self, tool_name: str, arguments: dict[str, Any] = None) -> dict:
        self._request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments or {},
            },
        }
        try:
            resp = requests.post(
                self.url,
                json=payload,
                timeout=self.timeout,
                headers={"Content-Type": "application/json"},
            )
            resp.raise_for_status()
            result = resp.json()
            if "error" in result:
                return {"error": result["error"]}
            return result.get("result", result)
        except requests.ConnectionError:
            return {"error": f"Connection refused: {self.url}"}
        except requests.Timeout:
            return {"error": f"Request timed out ({self.timeout}s)"}
        except Exception as e:
            return {"error": str(e)}

    def list_tools(self) -> list[dict]:
        self._request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": "tools/list",
        }
        try:
            resp = requests.post(self.url, json=payload, timeout=10)
            resp.raise_for_status()
            result = resp.json()
            return result.get("result", {}).get("tools", [])
        except Exception as e:
            console.print(f"[red]Failed to list tools: {e}[/red]")
            return []

    def health(self) -> Optional[dict]:
        try:
            health_url = self.url.rsplit("/", 1)[0] + "/health"
            resp = requests.get(health_url, timeout=5)
            return resp.json()
        except Exception:
            return None


# --- Ollama Client ---

OLLAMA_TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "c2_start_radio",
            "description": "Start the C2 SubGHz radio on the controller Flipper. Must be called before sending any commands.",
            "parameters": {
                "type": "object",
                "properties": {
                    "frequency": {
                        "type": "integer",
                        "description": "Frequency in Hz (default 433920000)",
                    }
                },
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ping_client",
            "description": "Ping the client Flipper over SubGHz to verify it's listening.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_hid_start",
            "description": "Start BLE HID profile on the client Flipper. It will appear as a Bluetooth keyboard to nearby devices. Target must pair with it.",
            "parameters": {
                "type": "object",
                "properties": {
                    "device_name": {
                        "type": "string",
                        "description": "BLE device name (max 8 chars, default 'FlpC2')",
                    }
                },
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_hid_type",
            "description": "Type text as keyboard input on the target device via BLE HID. Requires ble_hid_start first. Use \\n for Enter.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to type (printable ASCII, max 250 chars)",
                    }
                },
                "required": ["text"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_hid_press",
            "description": "Press a key combination on the target. Use '+' to combine modifiers. Examples: 'GUI+r', 'CTRL+SHIFT+ESC', 'ENTER'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key combo (e.g., 'GUI+r', 'CTRL+c', 'ENTER')",
                    }
                },
                "required": ["key"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_hid_stop",
            "description": "Stop BLE HID profile on client Flipper, release all keys, restore normal BLE.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_beacon_start",
            "description": "Start BLE beacon spoofing on client Flipper. Broadcasts custom advertisement data.",
            "parameters": {
                "type": "object",
                "properties": {
                    "adv_data_hex": {
                        "type": "string",
                        "description": "Hex-encoded BLE advertisement payload (2-62 hex chars = 1-31 bytes)",
                    },
                    "mac": {
                        "type": "string",
                        "description": "Spoofed MAC address in hex (12 chars, e.g., 'DEADBEEF0001')",
                    },
                    "interval_ms": {
                        "type": "integer",
                        "description": "Advertisement interval in ms (20-10240, default 100)",
                    },
                },
                "required": ["adv_data_hex"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "ble_beacon_stop",
            "description": "Stop BLE beacon broadcasting on client Flipper.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "stop_all",
            "description": "Stop all active BLE attacks (HID and beacon) on client Flipper.",
            "parameters": {"type": "object", "properties": {}},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "get_client_status",
            "description": "Get status of the client Flipper (heap, HID state, beacon state, RX/TX counts).",
            "parameters": {"type": "object", "properties": {}},
        },
    },
]


class OllamaClient:
    """Client for Ollama's chat API with tool-calling."""

    def __init__(self, url: str = "http://localhost:11434", model: str = "llama3.1"):
        self.url = url
        self.model = model

    def chat(self, messages: list[dict], tools: list[dict] = None) -> dict:
        payload = {
            "model": self.model,
            "messages": messages,
            "stream": False,
        }
        if tools:
            payload["tools"] = tools

        try:
            resp = requests.post(
                f"{self.url}/api/chat",
                json=payload,
                timeout=120,
            )
            resp.raise_for_status()
            return resp.json()
        except requests.ConnectionError:
            return {"error": f"Ollama not reachable at {self.url}"}
        except Exception as e:
            return {"error": str(e)}

    def is_available(self) -> bool:
        try:
            resp = requests.get(f"{self.url}/api/tags", timeout=5)
            return resp.status_code == 200
        except Exception:
            return False


# --- Tool Execution ---

def execute_tool(mcp: MCPClient, tool_name: str, args: dict) -> str:
    """Map high-level tool names to MCP C2 calls."""

    if tool_name == "c2_start_radio":
        freq = args.get("frequency", 433920000)
        result = mcp.call("c2_configure", {"action": "start", "frequency": freq})
        return format_result(result)

    elif tool_name == "ping_client":
        result = mcp.call("c2_ping", {})
        return format_result(result)

    elif tool_name == "ble_hid_start":
        name = args.get("device_name", "FlpC2")
        result = mcp.call("c2_send", {"command": "ble_hid_start", "payload": name})
        return format_result(result)

    elif tool_name == "ble_hid_type":
        text = args.get("text", "")
        if len(text) > 250:
            text = text[:250]
        result = mcp.call("c2_send", {"command": "ble_hid_type", "payload": text})
        return format_result(result)

    elif tool_name == "ble_hid_press":
        key = args.get("key", "")
        result = mcp.call("c2_send", {"command": "ble_hid_press", "payload": key})
        return format_result(result)

    elif tool_name == "ble_hid_stop":
        result = mcp.call("c2_send", {"command": "ble_hid_stop"})
        return format_result(result)

    elif tool_name == "ble_beacon_start":
        adv_hex = args.get("adv_data_hex", "")
        # Build binary payload: [adv_len][adv_data][mac][interval]
        adv_bytes = bytes.fromhex(adv_hex)
        if len(adv_bytes) < 1 or len(adv_bytes) > 31:
            return "Error: adv_data must be 1-31 bytes"

        payload = bytes([len(adv_bytes)]) + adv_bytes

        mac_hex = args.get("mac", "")
        if mac_hex and len(mac_hex) == 12:
            payload += bytes.fromhex(mac_hex)

        interval = args.get("interval_ms", 100)
        payload += bytes([(interval >> 8) & 0xFF, interval & 0xFF])

        # Send as hex string
        result = mcp.call("c2_send", {
            "command": "ble_beacon_start",
            "payload": payload.hex(),
        })
        return format_result(result)

    elif tool_name == "ble_beacon_stop":
        result = mcp.call("c2_send", {"command": "ble_beacon_stop"})
        return format_result(result)

    elif tool_name == "stop_all":
        r1 = mcp.call("c2_send", {"command": "ble_hid_stop"})
        r2 = mcp.call("c2_send", {"command": "ble_beacon_stop"})
        return f"HID: {format_result(r1)}\nBeacon: {format_result(r2)}"

    elif tool_name == "get_client_status":
        result = mcp.call("c2_send", {"command": "ble_hid_stop"})
        # Actually we want status, not hid_stop
        result = mcp.call("c2_status", {})
        return format_result(result)

    else:
        return f"Unknown tool: {tool_name}"


def format_result(result: dict) -> str:
    """Format MCP result for display."""
    if isinstance(result, dict):
        if "error" in result:
            return f"Error: {result['error']}"
        if "content" in result:
            content = result["content"]
            if isinstance(content, list):
                return "\n".join(
                    item.get("text", str(item)) for item in content
                )
            return str(content)
        return json.dumps(result, indent=2)
    return str(result)


# --- Interactive Mode ---

def run_interactive(mcp: MCPClient, ollama: Optional[OllamaClient]):
    """Interactive command loop."""
    console.print(Panel(
        "[bold green]C2 Agent Interactive Mode[/bold green]\n"
        "Commands: ping, status, hid start, hid type <text>, hid press <key>,\n"
        "          hid stop, beacon start <hex>, beacon stop, stop, auto <objective>,\n"
        "          tools, health, radio start, radio stop, quit",
        title="C2 Agent",
    ))

    while True:
        try:
            user_input = console.input("[bold cyan]c2>[/bold cyan] ").strip()
        except (EOFError, KeyboardInterrupt):
            break

        if not user_input:
            continue

        if user_input in ("quit", "exit", "q"):
            break

        elif user_input == "ping":
            result = execute_tool(mcp, "ping_client", {})
            console.print(f"[green]{result}[/green]")

        elif user_input == "status":
            result = mcp.call("c2_status", {})
            console.print(format_result(result))

        elif user_input == "health":
            h = mcp.health()
            console.print(json.dumps(h, indent=2) if h else "[red]Not reachable[/red]")

        elif user_input == "tools":
            tools = mcp.list_tools()
            table = Table(title="Available MCP Tools")
            table.add_column("Name")
            table.add_column("Description", max_width=60)
            for t in tools:
                table.add_row(t.get("name", "?"), t.get("description", "")[:60])
            console.print(table)

        elif user_input.startswith("radio start"):
            parts = user_input.split()
            freq = int(parts[2]) if len(parts) > 2 else 433920000
            result = execute_tool(mcp, "c2_start_radio", {"frequency": freq})
            console.print(f"[green]{result}[/green]")

        elif user_input == "radio stop":
            result = mcp.call("c2_configure", {"action": "stop"})
            console.print(format_result(result))

        elif user_input.startswith("hid start"):
            parts = user_input.split(maxsplit=2)
            name = parts[2] if len(parts) > 2 else "FlpC2"
            result = execute_tool(mcp, "ble_hid_start", {"device_name": name})
            console.print(f"[green]{result}[/green]")

        elif user_input.startswith("hid type "):
            text = user_input[9:]
            result = execute_tool(mcp, "ble_hid_type", {"text": text})
            console.print(f"[green]{result}[/green]")

        elif user_input.startswith("hid press "):
            key = user_input[10:]
            result = execute_tool(mcp, "ble_hid_press", {"key": key})
            console.print(f"[green]{result}[/green]")

        elif user_input == "hid stop":
            result = execute_tool(mcp, "ble_hid_stop", {})
            console.print(f"[green]{result}[/green]")

        elif user_input.startswith("beacon start "):
            hex_data = user_input[13:].strip()
            result = execute_tool(mcp, "ble_beacon_start", {"adv_data_hex": hex_data})
            console.print(f"[green]{result}[/green]")

        elif user_input == "beacon stop":
            result = execute_tool(mcp, "ble_beacon_stop", {})
            console.print(f"[green]{result}[/green]")

        elif user_input == "stop":
            result = execute_tool(mcp, "stop_all", {})
            console.print(f"[yellow]{result}[/yellow]")

        elif user_input.startswith("auto "):
            if not ollama:
                console.print("[red]Ollama not available[/red]")
                continue
            objective = user_input[5:]
            run_autonomous(mcp, ollama, objective)

        else:
            if ollama:
                # Ask Ollama for help
                console.print("[dim]Asking Ollama...[/dim]")
                resp = ollama.chat(
                    [{"role": "user", "content": user_input}],
                    tools=OLLAMA_TOOLS,
                )
                if "error" in resp:
                    console.print(f"[red]{resp['error']}[/red]")
                else:
                    msg = resp.get("message", {})
                    if msg.get("content"):
                        console.print(msg["content"])
                    if msg.get("tool_calls"):
                        for tc in msg["tool_calls"]:
                            fn = tc.get("function", {})
                            name = fn.get("name", "?")
                            args = fn.get("arguments", {})
                            console.print(f"[cyan]Tool: {name}({json.dumps(args)})[/cyan]")
                            result = execute_tool(mcp, name, args)
                            console.print(f"[green]{result}[/green]")
            else:
                console.print(f"[red]Unknown command: {user_input}[/red]")

    console.print("[dim]Goodbye.[/dim]")


# --- Autonomous Mode ---

SYSTEM_PROMPT = """You are a C2 agent controlling a Flipper Zero over SubGHz radio.
Your task is to execute BLE attacks on authorized target devices for security research.

Available actions via tool calls:
- Start/stop the SubGHz radio
- Ping the client Flipper
- Start BLE HID profile (keyboard/mouse emulation)
- Type text, press key combos, move/click mouse via BLE HID
- Start/stop BLE beacon spoofing
- Get client status

Important:
- Always start the radio first with c2_start_radio
- Always ping the client to confirm connectivity before attacks
- BLE HID requires the target to be paired with the client Flipper
- Be methodical: start HID, wait, then type/press keys
- Report results after each step
- Say DONE when the objective is complete"""


def run_autonomous(mcp: MCPClient, ollama: OllamaClient, objective: str):
    """Run Ollama in an autonomous agent loop."""
    console.print(Panel(f"[bold]Autonomous mode[/bold]\nObjective: {objective}", title="Auto"))

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": f"Objective: {objective}"},
    ]

    max_turns = 20
    for turn in range(max_turns):
        console.print(f"\n[dim]--- Turn {turn + 1}/{max_turns} ---[/dim]")

        resp = ollama.chat(messages, tools=OLLAMA_TOOLS)
        if "error" in resp:
            console.print(f"[red]Ollama error: {resp['error']}[/red]")
            break

        msg = resp.get("message", {})
        role = msg.get("role", "assistant")

        if msg.get("content"):
            console.print(f"[bold]{msg['content']}[/bold]")
            if "DONE" in msg["content"].upper():
                console.print("[green]Objective complete.[/green]")
                break

        messages.append(msg)

        if msg.get("tool_calls"):
            for tc in msg["tool_calls"]:
                fn = tc.get("function", {})
                name = fn.get("name", "?")
                args = fn.get("arguments", {})
                console.print(f"  [cyan]-> {name}({json.dumps(args)})[/cyan]")

                result = execute_tool(mcp, name, args)
                console.print(f"  [green]<- {result}[/green]")

                messages.append({
                    "role": "tool",
                    "content": result,
                })
        else:
            # No tool calls and no DONE — might be stuck
            if turn > 2:
                messages.append({
                    "role": "user",
                    "content": "Continue with the objective or say DONE if complete.",
                })

    console.print("[dim]Autonomous mode ended.[/dim]")


# --- Main ---

def main():
    parser = argparse.ArgumentParser(
        description="C2 Agent — Ollama-directed Flipper BLE attack controller"
    )
    parser.add_argument(
        "--mcp",
        required=True,
        help="MCP endpoint URL (e.g., http://192.168.1.100:8080/mcp)",
    )
    parser.add_argument(
        "--ollama-url",
        default="http://localhost:11434",
        help="Ollama API URL (default: http://localhost:11434)",
    )
    parser.add_argument(
        "--model",
        default="llama3.1",
        help="Ollama model name (default: llama3.1)",
    )
    parser.add_argument(
        "--auto",
        metavar="OBJECTIVE",
        help="Run in autonomous mode with the given objective",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=60,
        help="MCP request timeout in seconds (default: 60)",
    )

    args = parser.parse_args()

    mcp = MCPClient(args.mcp, timeout=args.timeout)

    # Check MCP connectivity
    health = mcp.health()
    if health:
        console.print(f"[green]MCP: connected ({health.get('status', '?')})[/green]")
        if health.get("device_connected"):
            console.print(f"[green]Device: {health.get('device_id', 'connected')}[/green]")
        else:
            console.print("[yellow]Device: not connected[/yellow]")
    else:
        console.print(f"[yellow]MCP: not reachable at {args.mcp}[/yellow]")

    # Check Ollama
    ollama = OllamaClient(args.ollama_url, args.model)
    if ollama.is_available():
        console.print(f"[green]Ollama: available (model: {args.model})[/green]")
    else:
        console.print("[yellow]Ollama: not available (interactive-only mode)[/yellow]")
        ollama = None

    if args.auto:
        if not ollama:
            console.print("[red]Autonomous mode requires Ollama. Exiting.[/red]")
            sys.exit(1)
        run_autonomous(mcp, ollama, args.auto)
    else:
        run_interactive(mcp, ollama)


if __name__ == "__main__":
    main()
