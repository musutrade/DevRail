# syntax=docker/dockerfile:1.7

FROM node:24.18.0-alpine3.23@sha256:595398b0081eacda8e1c4c5b97b76cd1020e4d58a8ebcb4843b9bca1e79e7436 AS builder
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

FROM nginx:1.30.4-alpine3.24@sha256:02b1b2a0445514891a14aa371845f6085d5d9d10d385b30d6aad606a50a29a05 AS runtime
RUN apk upgrade --no-cache
COPY deployment/nginx/nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /build/frontend/dist/arc-admin/browser /usr/share/nginx/html
USER 101:101
EXPOSE 8080 8443
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/nginx-healthz"]
ENTRYPOINT ["nginx", "-g", "daemon off;"]
