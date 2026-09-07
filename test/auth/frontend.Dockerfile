FROM node:22-alpine
RUN apk add --no-cache chromium font-noto
WORKDIR /checks
RUN npm install --no-audit --no-fund @playwright/test@1.51.1
COPY frontend /checks/frontend
RUN cd frontend && npm ci
COPY --chmod=644 logo.png /checks/frontend/public/logo.png
COPY test/auth/frontend.spec.cjs test/auth/playwright.config.cjs /checks/
CMD ["npx", "playwright", "test", "--config=playwright.config.cjs"]
