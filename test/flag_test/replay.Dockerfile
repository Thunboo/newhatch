FROM node:22-alpine
RUN apk add --no-cache bash curl python3 py3-requests nginx chromium font-noto
WORKDIR /checks
RUN npm install --no-audit --no-fund @playwright/test@1.51.1
COPY frontend/package.json frontend/package-lock.json frontend/
RUN npm --prefix frontend ci
COPY frontend frontend
RUN npm --prefix frontend run lint && npm --prefix frontend run build
COPY test/auth/*.spec.cjs test/auth/
COPY test/flag_test test/flag_test
RUN mkdir -p /usr/share/nginx/html && cp test/flag_test/html/flag.json /usr/share/nginx/html/flag.json
CMD ["npx", "playwright", "test", "--config=test/flag_test/playwright.config.cjs"]
