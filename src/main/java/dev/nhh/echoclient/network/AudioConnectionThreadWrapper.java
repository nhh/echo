package dev.nhh.echoclient.network;

import dev.nhh.echoclient.audio.microphone.MicrophonePacket;
import dev.nhh.echoclient.audio.speaker.AudioPacket;
import dev.nhh.echoclient.audio.speaker.SpeakerQueue;
import dev.nhh.echoclient.util.ThreadWrapper;

import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.UnknownHostException;
import java.util.Queue;

// Sending and Receiving audio packets
public class AudioConnectionThreadWrapper extends ThreadWrapper {

    private final Queue<AudioPacket> speakerQueue = SpeakerQueue.INSTANCE.getQueue();
    private final Queue<MicrophonePacket> audioConnectionQueue = AudioConnectionQueue.INSTANCE.getQueue();

    public AudioConnectionThreadWrapper(String name, boolean daemon) {
        super(name, daemon);
    }

    @Override
    public void run() {

        final InetAddress address;

        try {
            address = InetAddress.getByName("localhost");
        } catch (UnknownHostException e) {
            e.printStackTrace();
            return;
        }

        while(this.isRunning.get()) {

            final int numBytesRead;

            while(!this.audioConnectionQueue.isEmpty()) {

                final var data = audioConnectionQueue.poll();

                final var packet = new DatagramPacket(data.getData(), data.getBytesRead(), address, 4445);

                try {

                    final var socket = new DatagramSocket();
                    socket.send(packet);

                    final byte[] buffer = new byte[1024];
                    final var response = new DatagramPacket(buffer, buffer.length);
                    socket.receive(response);
                    speakerQueue.add(new AudioPacket(response.getData(), 0, response.getData().length));
                } catch (IOException e) {
                    e.printStackTrace();
                }

            }
        }
    }
}
