// Transparent byte forwarding to the REAL Rust Node; no HTTP parsing/replies.
const net=require('node:net');
const {Transform,pipeline}=require('node:stream');
function createTcpProfile({targetPort,delayMs=0,bytesPerSecond=0}){
  let disconnected=false;
  const sockets=new Set(),counts={connections:0,bytesToNode:0,bytesFromNode:0,maxConnections:0};
  function direction(name){let first=true;return new Transform({highWaterMark:65536,transform(chunk,_encoding,callback){const delay=(first?delayMs:0)+(bytesPerSecond?chunk.length*1000/bytesPerSecond:0);first=false;counts[name]+=chunk.length;if(delay>0)setTimeout(()=>callback(null,chunk),delay);else callback(null,chunk);}});}
  const server=net.createServer(client=>{
    if(disconnected){client.destroy();return;}
    counts.connections++;const upstream=net.connect({host:'127.0.0.1',port:targetPort});
    client.setNoDelay(true);upstream.setNoDelay(true);sockets.add(client);sockets.add(upstream);counts.maxConnections=Math.max(counts.maxConnections,sockets.size/2);
    const close=()=>{client.destroy();upstream.destroy();sockets.delete(client);sockets.delete(upstream);};client.on('error',close);upstream.on('error',close);client.on('close',close);upstream.on('close',close);
    pipeline(client,direction('bytesToNode'),upstream,()=>{});pipeline(upstream,direction('bytesFromNode'),client,()=>{});
  });
  return {setDisconnected(value){disconnected=!!value;counts.networkCuts=(counts.networkCuts||0)+(value?1:0);if(disconnected)for(const socket of sockets)socket.destroy();},async start(){await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});return server.address().port;},snapshot(){return {kind:'transparent TCP proxy',delayPerDirectionFirstChunkMs:delayMs,bytesPerSecondPerDirectionPerConnection:bytesPerSecond,scope:'Local proxy, no network propagation, TLS or loss. First application chunk of each TCP connection delayed; later keep-alive requests do not incur another artificial RTT. Rate is per connection, not an aggregate WAN link.',...counts,activeConnections:sockets.size/2};},async close(){for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}};
}
module.exports={createTcpProfile};
