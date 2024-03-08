import './style.css'
import { setupSign } from './sube'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div>
    <div class="card">
        <h4> Sube Demo </h4>
        <div class="row">
            <label for="mnonic">Wallet nmonic</label>
            <input id="mnomic" value="will merry consider pause public abuse truck lonely until enforce hat subject used master few pass useless fiction victory thunder struggle hover mushroom suggest">
        </div>
        <div class="row">
            <label for="uri">URI</label>
            <input id="uri" value="wss://rococo-rpc.polkadot.io/balances/transfer_Keep_Alive">
        </div>
        <div class="row">
            <label for="data">Body</label>
            <textarea id="data" name="textarea" rows="10" cols="50">
              {  
                "dest": {
                    "Id": "0x3c85f79f28628bee75cdb9eddfeae249f813fad95f84120d068fbc990c4b717d"
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
