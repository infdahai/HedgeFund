#!/bin/bash
# 找到昨天的日期
YESTERDAY=$(date -d "yesterday" '+%Y-%m-%d')
DB_FILE="/home/ubuntu/opinion-hft/data/opinion-$YESTERDAY.sqlite"

# 如果文件存在，且没有被压缩过
if [ -f "$DB_FILE" ]; then
    # 使用 zstd 压缩 (比 gzip 快且压缩率高)
    # 如果没装 zstd，sudo apt install zstd
    zstd --rm -9 "$DB_FILE"
    
    # 结果变成 opinion-2026-01-20.sqlite.zst
fi

# 删除超过 90 天的压缩文件，释放 AWS 空间
find /home/ubuntu/opinion-hft/data/ -name "*.zst" -mtime +90 -delete