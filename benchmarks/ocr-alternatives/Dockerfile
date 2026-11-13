FROM python:3.12-slim-bookworm@sha256:935c3c5b12357a7e10caa9c1e6ca5692344d97cf229832ef584be2cec9fc03df
RUN apt-get update && apt-get install -y --no-install-recommends libgomp1 libglib2.0-0 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY requirements-linux-arm64-*.txt /requirements/
RUN pip install --no-cache-dir -r /requirements/requirements-linux-arm64-rapid.txt
RUN python -m venv /opt/lite && /opt/lite/bin/pip install --no-cache-dir -r /requirements/requirements-linux-arm64-lite.txt
RUN python -m venv /opt/tesseract && /opt/tesseract/bin/pip install --no-cache-dir --only-binary=:all: -r /requirements/requirements-linux-arm64-tesseract.txt
ENV OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 VECLIB_MAXIMUM_THREADS=1 NUMEXPR_NUM_THREADS=1 HF_HUB_OFFLINE=1
WORKDIR /bench
