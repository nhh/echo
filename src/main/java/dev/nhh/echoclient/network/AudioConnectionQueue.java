package dev.nhh.echoclient.network;

import dev.nhh.echoclient.audio.microphone.MicrophonePacket;

import java.util.Queue;
import java.util.concurrent.ConcurrentLinkedQueue;

public enum AudioConnectionQueue {

    INSTANCE;

    private final Queue<MicrophonePacket> queue = new ConcurrentLinkedQueue<>();

    public Queue<MicrophonePacket> getQueue() {
        return queue;
    }

    public void add(MicrophonePacket packet) {
        this.queue.add(packet);
    }

}
