# 1. 修改内核参数
echo "net.core.default_qdisc=fq" | sudo tee -a /etc/sysctl.conf
echo "net.ipv4.tcp_congestion_control=bbr" | sudo tee -a /etc/sysctl.conf

# 2. 生效配置
sudo sysctl -p

# 3. 验证 (如果输出包含 bbr，说明成功)
sysctl net.ipv4.tcp_congestion_control
# 输出应为: net.ipv4.tcp_congestion_control = bbr

# 1. 临时生效 (当前会话)
ulimit -n 65535

# 2. 永久生效 (修改配置文件)
sudo bash -c 'cat <<EOF >> /etc/security/limits.conf
* soft nofile 65535
* hard nofile 65535
EOF'

# 1. 禁止空闲后的慢启动 (关键!)
echo "net.ipv4.tcp_slow_start_after_idle=0" | sudo tee -a /etc/sysctl.conf

# 2. 允许重用 TIME-WAIT 套接字 (提升连接效率)
echo "net.ipv4.tcp_tw_reuse=1" | sudo tee -a /etc/sysctl.conf

# 3. 增大 TCP 读写缓冲区 (应对突发数据包)
echo "net.core.rmem_max=16777216" | sudo tee -a /etc/sysctl.conf
echo "net.core.wmem_max=16777216" | sudo tee -a /etc/sysctl.conf

# 4. 生效配置
sudo sysctl -p

# 告诉内核：除非物理内存真的要爆了，否则绝对不要用 Swap
echo "vm.swappiness=10" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p

# -20 是最高优先级 (普通进程是 0)
sudo nice -n -20 ./opinion-hft