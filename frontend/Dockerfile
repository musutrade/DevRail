# syntax=docker/dockerfile:1.7

FROM node:24.18.0-alpine3.23 AS builder
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

FROM nginx:1.30.4-alpine3.24 AS runtime
RUN apk upgrade --no-cache
COPY deployment/nginx/nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /build/frontend/dist/arc-admin/browser /usr/share/nginx/html
USER 101:101
EXPOSE 8080 8443
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/nginx-healthz"]
ENTRYPOINT ["nginx", "-g", "daemon off;"]
