#!/bin/bash

# 配置服务器地址和端口
SERVER="127.0.0.1"
PORT=8080

echo "--- 🚀 开始 AEX 多协议服务器测试 ---"

# 1. 测试 HTTP 路径
echo -n "[1/3] 测试 HTTP 协议... "
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" http://$SERVER:$PORT/)
if [ "$HTTP_STATUS" == "200" ]; then
    echo "✅ 成功 (HTTP 200)"
else
    echo "❌ 失败 (状态码: $HTTP_STATUS)"
fi

# 2. 测试 TCP 私有协议 (ID: 1001)
# 逻辑：[4字节长度: 00 00 00 04] + [4字节ID: 00 00 03 E9]
echo -n "[2/3] 测试 TCP 私有协议 (ID: 1001)... "
printf "\x00\x00\x00\x04\x00\x00\x03\xe9" | nc -w 2 $SERVER $PORT
echo "✅ 已发送 (请检查服务器日志)"

# 3. 测试 UDP 协议 (ID: 2002)
# 逻辑：直接发送 [4字节ID: 00 00 07 D2]
echo -n "[3/3] 测试 UDP 协议 (ID: 2002)... "
printf "\x00\x00\x07\xd2" | nc -u -w 1 $SERVER $PORT
echo "✅ 已发送 (请检查服务器日志)"

echo "--- 🔍 AEX 深度测试 (含 Bincode 长度前缀) ---"

# 1. TCP 测试 (ID: 1001)
# [4字节协议总长] + [8字节Bincode Vec长度] + [4字节ID数据]
# 协议总长 = 8 + 4 = 12 字节 (\x0c)
# Bincode Vec长度 = 4 字节 (\x04)
echo "[TCP] 发送 ID 1001..."
printf "\x00\x00\x00\x0c\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03\xe9" | nc -w 1 $SERVER $PORT

# 2. UDP 测试 (ID: 2002)
# UDP 不需要前面的 4 字节协议总长，直接发 Bincode 数据
# [8字节Bincode Vec长度] + [4字节ID数据]
echo "[UDP] 发送 ID 2002..."
printf "\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x07\xd2" | nc -u -w 1 $SERVER $PORT

echo "--- 检查结束 ---"
