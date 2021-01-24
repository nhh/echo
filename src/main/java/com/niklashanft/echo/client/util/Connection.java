package com.niklashanft.echo.client.util;

import com.niklashanft.echo.client.audio.microphone.Microphone;
import com.niklashanft.echo.client.audio.speaker.Speaker;

import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class Connection {

    private DatagramSocket socket;
    private InetAddress address;
    private final Microphone microphone = new Microphone();
    private final Speaker speaker = new Speaker(UUID.randomUUID());

    private final AtomicBoolean isRunning = new AtomicBoolean(false);

    public Connection(String ip) {
        try {
            socket = new DatagramSocket();
            address = InetAddress.getByName(ip);
            microphone.start();

            var t = new Thread(() -> {
                while (true) {
                    var responseBuf = new byte[2200];
                    var response = new DatagramPacket(responseBuf, responseBuf.length);

                    try {
                        socket.receive(response);
                        speaker.play(response.getData());
                    } catch (IOException e) {
                        e.printStackTrace();
                    }
                }
            });

            t.start();

        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    public void send() {
        var buf = microphone.read();
        DatagramPacket packet = new DatagramPacket(buf, buf.length, address, 4445);

        try {
            socket.send(packet);
        } catch (IOException e) {
            e.printStackTrace();
        }

    }

    public void stop() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
    }

}
