package dev.nhh.echo.client.controller;

import javafx.fxml.FXML;
import javafx.scene.control.Button;

import java.util.Date;

public class MenuController {

    @FXML
    public Button button;

    @FXML
    void click() {
        button.setText("Hallo Welt" + new Date().toString());
    }

}
