package com.niklashanft.echo.server;

import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.util.HashMap;
import java.util.Map;

public class Server {

    private static final Map<InetAddress, Integer> clients = new HashMap<>();
    private static final byte[] buf = new byte[2200];

    private static DatagramSocket socket;

    public static void main(String[] args) {

        try {
            socket = new DatagramSocket(4445);
        } catch (IOException e) {
            e.printStackTrace();
        }

        while (true) {
            var packet = new DatagramPacket(buf, buf.length);

            try {
                socket.receive(packet);
            } catch (IOException e) {
                e.printStackTrace();
                continue;
            }

            var address = packet.getAddress();
            int port = packet.getPort();

            // Todo move this into a handshake packet or into a tcp command /join-channel
            clients.putIfAbsent(address, port);

            for (var entry : clients.entrySet()) {
                var p = new DatagramPacket(buf, buf.length, entry.getKey(), entry.getValue());

                try {
                    // Todo if a single client is unavailable atm the whole process is blocked.
                    // So this has to be called within a thread with a timeout of 250ms or what
                    socket.send(p);
                } catch (IOException e) {
                    e.printStackTrace();
                }
            }

        }
    }

}
