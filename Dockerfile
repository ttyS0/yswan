FROM ubuntu:impish AS base

ENV TZ=Asia/Shanghai \
    DEBIAN_FRONTEND=noninteractive
ENV LANG en_US.UTF8

RUN sed -i -E 's#https?://[^/]+#http://mirror.sjtu.edu.cn#g' /etc/apt/sources.list \
    && apt update && apt install -y locales \
    && localedef -i en_US -c -f UTF-8 -A /usr/share/locale/locale.alias en_US.UTF-8 \
    && apt update \
    && apt install -y tzdata \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
    && echo ${TZ} > /etc/timezone \
    && dpkg-reconfigure --frontend noninteractive tzdata \
    && apt install -y curl 'inetutils-*' net-tools iptables iproute2 tcpdump python3 \
    && rm -rf /var/lib/apt/lists/*

FROM base AS client

FROM base AS gateway

RUN apt update \
    && apt install -y dnsmasq \
    && rm -rf /var/lib/apt/lists/*

FROM php:apache AS web

RUN apt update && apt install -y curl 'inetutils-*' net-tools iptables iproute2 tcpdump python3 && rm -rf /var/lib/apt/lists/*
