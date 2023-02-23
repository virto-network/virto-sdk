import { sube_js } from 'sube-js'

export function setupCounter(element: HTMLButtonElement) {
  let counter = 0
  const setCounter = async (count: number) => {
    const f = await sube_js("http://localhost:8010/balances/transfer", {
      "dest": {
        "Id": "0xd4b2988bdfb59e9c3e6a1349f08de78fa20323206b86716742900fa3da332826",
      },
      "value": 1000,
    }, function() {

    });
    console.log({ f });
    counter = count
    element.innerHTML = `count is ${counter}`
  }

  element.addEventListener('click', () => setCounter(counter + 1))
  // setCounter(0)
}
