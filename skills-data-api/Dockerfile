FROM node:20-alpine

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci --omit=dev

COPY index.js ./
COPY skills.json ./
COPY load-mongo.sh ./

# Skills directory (can be mounted as Railway volume at /skills)
RUN mkdir -p /skills

ENV PORT=3100
ENV SKILLS_DIR=/skills
ENV NODE_ENV=production

EXPOSE 3100

CMD ["node", "index.js"]
