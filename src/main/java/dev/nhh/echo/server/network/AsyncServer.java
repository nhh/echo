package dev.nhh.echo.server.network;

import java.io.IOException;
import java.net.*;
import java.util.ArrayList;
import java.util.HashMap;

public class AsyncServer {

    private DatagramSocket socket;
    private ArrayList<SocketAddress> sockets;
    private byte[] buf = new byte[256];
    private boolean running;

    AsyncServer() {
        try {
            socket = new DatagramSocket(4445);
        } catch (SocketException e) {
            e.printStackTrace();
        }
    }

    public void run() {
        while (running) {
            DatagramPacket packet = new DatagramPacket(buf, buf.length);

            try {
                socket.receive(packet);
            } catch (IOException e) {
                e.printStackTrace();
            }

            String received = new String(packet.getData(), 0, packet.getLength());

            if (received.equals("HANDSHAKE_RECEIVER")) {
                this.sockets.add(packet.getSocketAddress());
            } else {
                sockets.forEach((client) -> {
                    try {
                        socket.send(new DatagramPacket(packet.getData(), packet.getLength(), client));
                    } catch (IOException e) {
                        e.printStackTrace();
                    }
                });
            }

        }
    }

}
