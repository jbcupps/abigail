// Synthetic OpenAI-compatible fixture for contract checks, not a real model.
import http from 'node:http';
import { appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export async function startModelFixture({ port = 0, logPath } = {}) {
  const model = 'abigail-mvp-contract-fixture';
  const server = http.createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname;
      if (request.method === 'GET' && pathname === '/v1/models') {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ object: 'list', data: [{ id: model, object: 'model', owned_by: 'test-fixture' }] }));
        return;
      }
      if (request.method !== 'POST' || pathname !== '/v1/chat/completions') {
        response.writeHead(404); response.end('Fixture endpoint not found'); return;
      }
      let body = '';
      for await (const chunk of request) {
        body += chunk;
        if (body.length > 2_000_000) throw new Error('Fixture request exceeded size limit');
      }
      const data = JSON.parse(body);
      const user = [...(data.messages ?? [])].reverse().find(message => message.role === 'user');
      const content = `Contract fixture reply: ${typeof user?.content === 'string' ? user.content : 'hello'}`;
      if (logPath) appendFileSync(logPath, JSON.stringify({ at: new Date().toISOString(), stream: Boolean(data.stream), model: data.model, user: user?.content }) + '\n');
      const common = { id: 'mvp-contract-completion', created: 1, model };
      if (data.stream) {
        response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
        for (let start = 0; start < content.length; start += 13) {
          response.write(`data: ${JSON.stringify({ ...common, object: 'chat.completion.chunk', choices: [{ index: 0, delta: { content: content.slice(start, start + 13) }, finish_reason: null }] })}\n\n`);
        }
        response.end(`data: ${JSON.stringify({ ...common, object: 'chat.completion.chunk', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\ndata: [DONE]\n\n`);
      } else {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ ...common, object: 'chat.completion', choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } }));
      }
    } catch (error) {
      response.writeHead(400, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: error.message }));
    }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
  return { url: `http://127.0.0.1:${server.address().port}`, model, server,
    close: () => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const fixture = await startModelFixture({ port: Number(process.argv[2] ?? 0) });
  process.stdout.write(JSON.stringify({ fixture: 'synthetic contract model', url: fixture.url, model: fixture.model }) + '\n');
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, async () => { await fixture.close(); process.exit(0); });
}
