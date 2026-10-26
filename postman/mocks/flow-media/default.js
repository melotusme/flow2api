// Local media transport fixture derived from the archived media archive contract.
// No Google authentication, inference or captcha behavior is simulated here.
const http = require('http');
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a94sAAAAASUVORK5CYII=', 'base64');
http.createServer((req, res) => {
  // @endpoint GET /image/success
  if (req.method === 'GET' && req.url === '/image/success') {
    res.writeHead(200, { 'Content-Type': 'image/png' });
    return res.end(png);
  }
  // @endpoint GET /image/error
  if (req.method === 'GET' && req.url === '/image/error') {
    res.writeHead(503, { 'Content-Type': 'text/html' });
    return res.end('<html>Temporary upstream failure</html>');
  }
  // @endpoint GET /image/html
  if (req.method === 'GET' && req.url === '/image/html') {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    return res.end('<html>Login required</html>');
  }
  res.writeHead(501, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ error: 'Not modeled by media transport fixture' }));
}).listen(process.env.PORT || 4951);
