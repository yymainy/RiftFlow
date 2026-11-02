FROM node:24-alpine
WORKDIR /app
COPY --chown=node:node server.mjs ai-config.mjs seed.json package.json admin.html ./
USER node
ENV HOST=0.0.0.0 PORT=8080 DATA_DIR=/data
EXPOSE 8080
CMD ["node", "server.mjs"]
