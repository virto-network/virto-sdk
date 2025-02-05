import './style.css'
import { setupSign } from './sube'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div>
    <div class="card">
        <h4> Sube Demo </h4>
        <div class="row">
            <label for="mnonic">Wallet nmonic</label>
            <input id="mnomic" value="">
        </div>
        <div class="row">
            <label for="uri">URI</label>
            <input id="uri" value="https://kreivo.io/balances/transfer_keep_alive">
        </div>
        <div class="row">
            <label for="data">Body</label>
            <textarea id="data" name="textarea" rows="10" cols="50">
              {
                  "dest": {
                      "Id": "0x28f8eba6bdaecef86bc33c0f7ba0ccfc77664776a6033c31c7716f4de2a1c74f"
                  },
                  "value": 100000
              }
            </textarea>
        </div>
        <button id="counter" type="button"> Submit </button>
    </div>
  </div>
`

setupSign(document.querySelector<HTMLButtonElement>('#counter')!)
