FROM node:22-alpine AS builder
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json frontend/
RUN cd frontend && npm ci
COPY frontend frontend
COPY --chmod=644 logo.png frontend/public/logo.png
RUN cd frontend && npm run build

FROM nginx:1.27-alpine
RUN apk add --no-cache python3
COPY docker/nginx.conf /etc/nginx/newhatch.conf.template
COPY --chmod=755 docker/configure-nginx.py /docker-entrypoint.d/40-configure-nginx.sh
COPY --from=builder /app/frontend/dist /usr/share/nginx/html
EXPOSE 8080
