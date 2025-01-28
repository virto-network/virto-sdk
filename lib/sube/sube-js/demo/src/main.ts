import './style.css'
import { setupSign } from './sube'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div>
    <div class="card">
        <h4> Sube Demo </h4>
        <div class="row">
            <label for="mnonic">Wallet nmonic</label>
            <input id="mnomic">
        </div>
        <div class="row">
            <label for="uri">URI</label>
            <input id="uri">
        </div>
        <div class="row">
            <label for="data">Body</label>
            <textarea id="data" name="textarea" rows="10" cols="50">
              {  
                "transfer": {
                  "dest": {
                      "Id": "0x12840f0626ac847d41089c4e05cf0719c5698af1e3bb87b66542de70b2de4b2b"
                  },
                  "value": 100000
                }
              }
            </textarea>
        </div>
        <button id="counter" type="button"> Submit </button>
    </div>
  </div>
`

setupSign(document.querySelector<HTMLButtonElement>('#counter')!)
