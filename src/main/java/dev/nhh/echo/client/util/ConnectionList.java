package dev.nhh.echo.client.util;

import java.util.ArrayList;

public enum ConnectionList {
    INSTANCE;

    private final ArrayList<Connection> connections = new ArrayList<>();


    public void addConnection(Connection connection) {
        if(this.connections.contains(connection)) {
            return;
        }
        this.connections.add(connection);
    }

    public void stopAllConnections() {
        this.connections.forEach(Connection::shutdown);
    }

}
