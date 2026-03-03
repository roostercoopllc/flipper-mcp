# c2-agent — Flipper MCP C2 scenario demo container
# For authorized security research only.

FROM python:3.11-slim AS base

# cairosvg needs cairo system library for SVG→PNG conversion
RUN apt-get update && apt-get install -y --no-install-recommends \
    libcairo2 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY *.py .

# Runtime config — override via ConfigMap in k8s or -e flags in docker run
ENV FLIPPER_HOST=192.168.0.58
ENV FLIPPER_PORT=8080
ENV OLLAMA_URL=http://192.168.0.167:11434
ENV OLLAMA_MODEL=llama3.2
ENV DEMO_MODE=mock
ENV OUTPUT_DIR=/output

# Output volume: mount here to retrieve demo_output.svg / demo_output.png
VOLUME ["/output"]

CMD ["python", "demo_scenario.py"]
