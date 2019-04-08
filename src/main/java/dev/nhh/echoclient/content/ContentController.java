package dev.nhh.echoclient.content;

import javafx.fxml.FXML;
import javafx.scene.control.Button;

import java.util.Date;

public class ContentController {

    @FXML
    public Button button;

    @FXML
    void click() {
        button.setText("Hallo Welt" + new Date().toString());
    }

}
